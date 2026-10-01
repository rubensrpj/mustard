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
//! menos forte: um código de item devolve aquele item, o nome de um comando
//! (`map search`) devolve as chamadas dele, e qualquer outro termo passa pela
//! busca por nota, que não exige que o item tenha todas as palavras.
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
//! Cinco leituras respondem o que antes saía de script escrito na hora, e
//! nelas o `--term` não vale:
//!
//! - `wave-list` traz uma linha curta por onda, na ordem do número: o código,
//!   o número, a primeira linha do texto e as ondas de que ela depende. É o
//!   que quem conduz tirava do bloco `waves` inteiro, que traz também as
//!   tarefas, os pedidos e as entregas de todas as ondas.
//! - `item-<código>` traz a versão vigente do item, e `item-<número>`, aquela
//!   versão; as duas com `changed`, o que mudou da versão anterior. O item
//!   removido vem com `removed`, e a versão já substituída, com
//!   `replaced_by`. A mensagem do usuário sai com o texto inteiro, como
//!   qualquer outro item.
//! - `delivered-<n>` traz a entrega vigente da onda: a que a rodada assumiu
//!   ou, sem ela, a volta do agente. Do `agreed`, só o que não foi cumprido.
//! - `backlog` traz as tarefas ainda por entregar, uma por linha, e no fim o
//!   total de tarefas e de arquivos.
//! - `request-<n>` devolve, em texto puro, o pedido gravado no último envio da
//!   onda, igual byte a byte: é por ele que o agente lê o próprio pedido.
//!   `request-review` faz o mesmo com o último envio do revisor final, que
//!   não tem onda: o fechamento manda o revisor ler o pedido por ele.
//!
//! `calls` soma as chamadas de cada comando, uma linha por comando: quantas,
//! as falhas por motivo, a mediana, o p90 e o pior do tempo da chamada e do
//! tempo do filtro, e os tokens e o custo somados. O `--term` nela é o nome do
//! comando, e a linha dele vem só.
//!
//! O bloco `state` traz também o número da última mensagem do usuário, sem o
//! texto, que é o `origin` de quem grava a partir dela.
//!
//! **O painel sem termo vem sem o texto longo.** `metrics`, lido sem `--term`,
//! traz o envio, a injeção e a entrega sem o `text`: o texto que o gancho
//! colocou na conversa e o relato da entrega eram mais da metade da saída, e
//! quem lê o painel olha o comando, o tempo, as fichas e o motivo do bloqueio,
//! nunca esses textos. O campo `chars` diz o tamanho da injeção, e o texto
//! inteiro de cada uma sai por `item-<código>`; o da entrega, também por
//! `delivered-<n>`. Com `--term`, o leitor pede o conteúdo, e o texto vem.
//! `waves` segue com tudo: quem conduz lê ali o `text`, os `files`, o
//! `done_when` e o `agreed`; para só escolher a ordem das ondas, basta a
//! `wave-list`.
//!
//! **Quem trabalha na cópia de um pedido lê a linha do item sem as marcas do
//! binário e sem as palavras de busca.** De dentro da cópia de um pedido
//! aberto, `item-<código>`, `item-<número>` e `dispatch-<n>` deixam de fora
//! `v`, `at`, `author` e `keys` — e o `author` e o `keys` de dentro de
//! `changed` —, que ninguém lê ali: nenhuma instrução manda o agente usá-los,
//! o item novo que ele grava leva só `title`, `text` e `agent`, e nas
//! conversas guardadas as palavras de busca de um item lido por agente não
//! voltam em nada que ele escreve. Quem conduz a spec, de fora da cópia, lê a
//! linha inteira, porque a versão nova de um item repete o `keys` dele. A
//! mensagem do usuário guarda o `at` e o `author`: dizem quem falou e quando.
//! O `id`, o `origin`, o `replaces` e o `covers` ficam: são números que se
//! seguem com `item-<número>`.
//!
//! **A leitura do pedido fica registrada.** Quem lê `item-<código>`,
//! `item-<número>`, `lessons --term <número>` ou `dispatch-<n>` de dentro da
//! cópia de um pedido aberto (a pasta atual é a vaga gravada no envio, ou
//! uma pasta dentro dela) deixa uma chamada `read` por item achado, com o
//! pedido e o item (`read_record`). A leitura de outra pasta, e a que não
//! acha nada, não grava nada. É dessas chamadas que a entrega da onda e o
//! veredito da revisão final conferem que o agente leu tudo o que o pedido
//! lista.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use mustard_core::domain::lessons::kept;
use mustard_core::domain::mustard_id;
use mustard_core::domain::normalize::Languages;
use mustard_core::domain::spec_events::{
    calls_command, found_by, shown_line, Block, BlockQuery, EventRef, Hidden, ReadQuery, Refusal, SpecEvent,
    SpecLog,
};
use mustard_core::domain::spec_state::SpecState;
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt::{lesson_bank, request_items};
use mustard_core::platform::i18n::Locale;
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};

use super::read_record::{self, Request};
use crate::shared::spec_state::{checkout, session_from_env, DiskSpecState};

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
/// do ambiente, e a pasta de onde se lê, da pasta atual. Nunca entra em
/// pânico.
pub(crate) fn read_at(opts: &ReadOpts) -> Result<String, Value> {
    let from = std::env::current_dir().unwrap_or_else(|_| opts.root.clone());
    read_for(opts, session_from_env().as_deref(), &from)
}

/// [`read_at`] com a sessão e a pasta recebidas, que é como um teste as
/// escolhe. A pasta `from` diz de qual pedido é a leitura: dentro da cópia de
/// um pedido aberto, o que a leitura acha fica registrado como lido.
pub(crate) fn read_for(opts: &ReadOpts, session: Option<&str>, from: &Path) -> Result<String, Value> {
    let started = Instant::now();
    let project = super::project(&opts.root);
    let lang = project.lang;
    let refuse = move |refusal: Refusal| super::refused(&refusal, lang);

    let block = opts.block.trim();
    if block == "lessons" {
        let (report, found) = read_lessons(&project.root, opts.spec.as_deref(), opts.term.as_deref(), lang)?;
        if !found.is_empty()
            && let Some((spec, log)) = quiet_spec(opts, session, &project.root)
        {
            let read: Vec<String> = found.iter().map(|id| format!("lesson-{id}")).collect();
            let request = read_record::request_of(&log, from);
            note_reading(&project.root, &spec, session, request.as_ref(), None, &read, started);
        }
        return Ok(report);
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
    // Quem lê de dentro da cópia de um pedido aberto lê para implementar.
    let request = read_record::request_of(&log, from);
    let item_view = if request.is_some() { Shown::Agent } else { Shown::Whole };
    // O que a leitura dá além dos eventos, depois da lista.
    let mut extra: Vec<(&str, Value)> = Vec::new();
    // O que a leitura achou do pedido, para o registro da leitura, e a onda a
    // que ele se restringe: o `dispatch-<n>` só conta na cópia da onda `n`.
    let mut read: Vec<String> = Vec::new();
    let mut only_wave: Option<u64> = None;
    let events: Vec<String> = match reading {
        ReadQuery::Request(wave) => return Ok(request_text(&log, wave)),
        ReadQuery::ReviewRequest => return Ok(review_request_text(&log)),
        ReadQuery::Dispatch(wave) => {
            let (lines, listed) =
                dispatch_lines(&project.root, &log, wave, term, &codes, &project.languages, item_view);
            read = listed;
            only_wave = Some(wave);
            lines
        }
        ReadQuery::Item(target) => {
            let found = find_item(&log, &target, &codes);
            read.extend(found.map(|event| codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string())));
            found.map(|event| item_line(&log, event, &codes, item_view)).into_iter().collect()
        }
        ReadQuery::WaveList => wave_list_lines(&log, &codes),
        ReadQuery::Delivered(wave) => delivered_lines(&log, wave, &codes),
        ReadQuery::Backlog => {
            let (lines, total) = backlog_lines(&log, &codes);
            extra.push(("total", total));
            lines
        }
        ReadQuery::Calls => calls_lines(&log, term),
        ReadQuery::Block(query) => {
            if query == BlockQuery::Block(Block::State)
                && let Some(id) = last_user_message(&log)
            {
                extra.push(("last_user_message", json!(id)));
            }
            // O pedido de cada envio é o texto maior da spec: a lista das
            // ondas e o painel mostram o envio sem ele, e só a leitura de uma
            // onda o traz inteiro. O painel sem termo tira também o texto da
            // injeção e da entrega.
            let shown = match query {
                BlockQuery::Block(Block::Metrics) if term.trim().is_empty() => Shown::Panel,
                BlockQuery::Block(Block::Waves | Block::Metrics) => Shown::Plan,
                _ => Shown::Whole,
            };
            let found = found_by(log.block(query), term, &codes, &project.languages);
            let found = if query == BlockQuery::Block(Block::State) { latest_copies(found) } else { found };
            found.into_iter().map(|e| shown_with_code(e, &codes, shown)).collect()
        }
    };
    note_reading(&project.root, &spec, session, request.as_ref(), only_wave, &read, started);
    let warnings: Vec<String> = log.skipped.iter().map(|s| s.message(lang)).collect();
    Ok(render(&spec, block, &events, &extra, &warnings))
}

/// Do estado, só a cópia mais nova de cada página: cada rodada grava uma
/// cópia nova, e a anterior fica sem leitor, porque a seguinte parte da
/// última. Sem esse corte, o estado de uma spec longa trazia centenas de
/// cópias, mais de cem mil letras, para quem só queria a fase e a branch. Os
/// outros eventos do estado seguem todos, na ordem do arquivo.
fn latest_copies(events: Vec<&SpecEvent>) -> Vec<&SpecEvent> {
    let mut newest: BTreeMap<Option<&str>, u64> = BTreeMap::new();
    for event in events.iter().filter(|event| event.event_type == "copy") {
        let id = newest.entry(event.str_field("page")).or_insert(event.id);
        *id = (*id).max(event.id);
    }
    events
        .into_iter()
        .filter(|event| event.event_type != "copy" || newest.get(&event.str_field("page")) == Some(&event.id))
        .collect()
}

/// A spec da leitura e o arquivo dela, sem recusar: a leitura das lições não
/// exige spec, e só o registro da leitura precisa dela. Sem spec, sem arquivo
/// ou com o arquivo ilegível, `None`.
fn quiet_spec(opts: &ReadOpts, session: Option<&str>, root: &Path) -> Option<(String, SpecLog)> {
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => DiskSpecState::new(&checkout(&opts.root)).active(session)?,
    };
    let log = store::read(&store::spec_file(root, &spec).ok()?).ok()??;
    Some((spec, log))
}

/// Registra o que a leitura achou como lido, quando ela veio da cópia de um
/// pedido aberto da spec (`request`, de `read_record::request_of`): uma
/// chamada por item. Com `only_wave`, só vale na cópia dessa onda. Sem item
/// achado, ou fora de uma cópia de pedido, nada é gravado.
fn note_reading(
    root: &Path,
    spec: &str,
    session: Option<&str>,
    request: Option<&Request>,
    only_wave: Option<u64>,
    read: &[String],
    started: Instant,
) {
    if read.is_empty() {
        return;
    }
    let Some(request) = request else { return };
    if only_wave.is_some_and(|wanted| request.wave != Some(wanted)) {
        return;
    }
    read_record::record(root, spec, session, request, read, started);
}

/// O pedido gravado no último envio da onda `wave`, igual byte a byte, em
/// texto puro. A onda sem envio não tem pedido: o texto vem vazio.
fn request_text(log: &SpecLog, wave: u64) -> String {
    let sent = log.last_by_wave("send").get(&wave).and_then(|id| log.get(*id));
    sent.and_then(|send| send.str_field("text")).unwrap_or_default().to_string()
}

/// O pedido gravado no último envio do revisor final, igual byte a byte, em
/// texto puro. O envio dele não tem onda, então `request-<n>` não o alcança.
/// Sem envio de revisão na spec, o texto vem vazio.
fn review_request_text(log: &SpecLog) -> String {
    let sent = log
        .visible()
        .into_iter()
        .filter(|event| event.event_type == "send" && event.str_field("role") == Some("review"))
        .max_by_key(|event| event.id);
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

/// O item que `item-<código>` ou `item-<número>` pede. Pelo código, a versão
/// vigente — ou, com o item todo removido, a última versão dele; pelo
/// número, aquela versão. Sem item, `None`.
fn find_item<'a>(log: &'a SpecLog, target: &EventRef, codes: &BTreeMap<u64, String>) -> Option<&'a SpecEvent> {
    let hidden = log.hidden();
    match target {
        EventRef::Id(id) => log.get(*id),
        EventRef::Code(code) => {
            let versions: Vec<&SpecEvent> = log.events.iter().filter(|e| codes.get(&e.id) == Some(code)).collect();
            versions.iter().rev().find(|e| !hidden.contains_key(&e.id)).or(versions.last()).copied()
        }
    }
}

/// A linha de um item, como `item-<código>` e `item-<número>` a mostram: os
/// campos dele, com o código, com `removed` quando uma remoção o tirou, com
/// `replaced_by` quando outra versão já o substitui, e com `changed` quando
/// ele substitui uma anterior. A mensagem do usuário sai como os outros
/// itens, com o texto inteiro. `shown` diz se é a linha de quem lê da cópia
/// de um pedido ([`Shown::Agent`]).
fn item_line(log: &SpecLog, event: &SpecEvent, codes: &BTreeMap<u64, String>, shown: Shown) -> String {
    let hidden = log.hidden();
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
    if shown == Shown::Agent {
        without_agent_omissions(&mut fields, &event.event_type);
    }
    shown_line(&fields)
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

/// As ondas que a leitura mostra, uma linha curta por onda, na ordem do
/// número: a versão vigente, o código, o número, a primeira linha não vazia
/// do texto e as ondas de que ela depende, vazia quando não depende de
/// nenhuma. A onda removida ou substituída fica de fora, como no bloco.
fn wave_list_lines(log: &SpecLog, codes: &BTreeMap<u64, String>) -> Vec<String> {
    let mut waves: Vec<&SpecEvent> =
        log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "wave").collect();
    waves.sort_by_key(|wave| (wave.wave(), wave.id));
    waves
        .into_iter()
        .map(|wave| {
            let text = wave.str_field("text").unwrap_or_default();
            let first_line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
            let mut line = Map::new();
            line.insert("id".into(), json!(wave.id));
            if let Some(code) = codes.get(&wave.id) {
                line.insert("code".into(), json!(code));
            }
            line.insert("n".into(), json!(wave.wave()));
            line.insert("first_line".into(), json!(first_line));
            line.insert("depends_on".into(), json!(wave.ints("depends_on")));
            shown_line(&line)
        })
        .collect()
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

/// A soma das chamadas de cada comando, uma linha por comando, na ordem do
/// nome: quantas são; as falhas por motivo, a recusa pelo motivo dela (ou
/// `refused`, quando ela não diz) e a falha do filtro, em que a busca volta
/// pelo banco, pelo filtro e o motivo (`jev:busy`); a mediana, o p90 e o pior
/// do tempo da chamada (`ms`) e do tempo do filtro (`filter_ms`); e os tokens
/// e o custo, somados. Os campos que nenhuma chamada do comando traz ficam de
/// fora. Com `term`, só o comando que tem esse nome.
fn calls_lines(log: &SpecLog, term: &str) -> Vec<String> {
    let term = term.trim();
    let mut by_command: BTreeMap<&str, Vec<&SpecEvent>> = BTreeMap::new();
    for call in log.block(BlockQuery::Block(Block::Metrics)) {
        if call.event_type == "call" && (term.is_empty() || calls_command(call, term)) {
            by_command.entry(call.str_field("command").unwrap_or_default().trim()).or_default().push(call);
        }
    }
    by_command.into_iter().map(|(command, calls)| shown_line(&calls_sum(command, &calls))).collect()
}

/// A linha de [`calls_lines`] de um comando.
fn calls_sum(command: &str, calls: &[&SpecEvent]) -> Map<String, Value> {
    let mut sum = Map::new();
    sum.insert("command".into(), json!(command));
    sum.insert("count".into(), json!(calls.len()));
    let mut failures: BTreeMap<&str, u64> = BTreeMap::new();
    for call in calls {
        let reason = if call.str_field("result") == Some("refused") {
            Some(call.str_field("refusal").unwrap_or("refused"))
        } else {
            call.str_field("filter").filter(|filter| filter.contains(':'))
        };
        if let Some(reason) = reason {
            *failures.entry(reason).or_default() += 1;
        }
    }
    if !failures.is_empty() {
        sum.insert("failures".into(), json!(failures));
    }
    let numbers = |field: &str| calls.iter().filter_map(|call| call.int(field)).collect::<Vec<u64>>();
    for field in ["ms", "filter_ms"] {
        if let Some(spread) = spread(numbers(field)) {
            sum.insert(field.into(), spread);
        }
    }
    for field in ["tokens", "cost_micro_usd"] {
        let values = numbers(field);
        if !values.is_empty() {
            sum.insert(field.into(), json!(values.iter().fold(0_u64, |total, n| total.saturating_add(*n))));
        }
    }
    sum
}

/// A mediana, o p90 e o pior de `values`, pela posição mais próxima: o
/// percentil `p` é o valor na posição ⌈n·p/100⌉ da lista em ordem crescente,
/// sem média entre dois valores. `None` sem valor nenhum.
fn spread(mut values: Vec<u64>) -> Option<Value> {
    values.sort_unstable();
    let at = |percent: usize| values.get((values.len() * percent).div_ceil(100).saturating_sub(1)).copied();
    Some(json!({"median": at(50)?, "p90": at(90)?, "max": values.last()?}))
}

/// O que o pedido da onda `wave` lista, pela mesma conta que o monta
/// ([`request_items`], com a escolha gravada no envio): primeiro os itens da
/// spec, cada um com o código, depois as lições do banco do projeto `root`,
/// pelo número delas. Com `term`, um código de item acha só aquele item, e
/// qualquer outro termo passa pela busca por nota nos itens e nas lições,
/// nas línguas `languages`. A onda que o plano não tem não tem pedido: a
/// lista vem vazia. Devolve as linhas e, ao lado, o que cada uma é para o
/// registro da leitura: o código do item e `lesson-<número>` da lição.
fn dispatch_lines(
    root: &Path,
    log: &SpecLog,
    wave: u64,
    term: &str,
    codes: &BTreeMap<u64, String>,
    languages: &Languages,
    shown: Shown,
) -> (Vec<String>, Vec<String>) {
    if !log.planned_waves().contains(&wave) {
        return (Vec::new(), Vec::new());
    }
    let bank = lesson_bank(root);
    let found = request_items(log, bank.as_ref(), wave, None, languages);
    let items = found_by(found.items, term, codes, languages);
    // A lição não tem código de item, e o número dela no banco pode ser o de
    // um item da spec: ela passa só pela busca, nunca pelos códigos da spec.
    let lessons = found_by(found.lessons, term, &BTreeMap::new(), languages);
    let read: Vec<String> = items
        .iter()
        .map(|item| codes.get(&item.id).cloned().unwrap_or_else(|| item.id.to_string()))
        .chain(lessons.iter().map(|lesson| format!("lesson-{}", lesson.id)))
        .collect();
    let mut lines: Vec<String> = items.into_iter().map(|e| shown_with_code(e, codes, shown)).collect();
    lines.extend(lessons.into_iter().map(|lesson| shown_line(&lesson.fields)));
    (lines, read)
}

/// O bloco `lessons`: fora da spec, no banco do projeto. `--term` é o número
/// da lição no banco — não uma busca, como no resto dos blocos —, porque é
/// assim que o pedido de uma onda a leva, sem copiar o texto dela. Sem
/// número, ou sem lição vigente com esse número, a lista vem vazia; sem
/// banco no disco, o mesmo. Devolve a saída e o número das lições achadas.
fn read_lessons(
    root: &Path,
    spec: Option<&str>,
    term: Option<&str>,
    lang: Locale,
) -> Result<(String, Vec<u64>), Value> {
    let refuse = move |refusal: Refusal| super::refused(&refusal, lang);
    let paths = ClaudePaths::for_project(root).map_err(|e| refuse(Refusal::Io { detail: e.to_string() }))?;
    let bank = mustard_core::io::lessons::read(&paths.lessons_path()).map_err(refuse)?.unwrap_or_default();
    let wanted = term.and_then(|t| t.trim().parse::<u64>().ok());
    let found: Vec<_> = kept(&bank).into_iter().filter(|lesson| wanted.is_some_and(|id| lesson.id == id)).collect();
    let events: Vec<String> = found.iter().map(|lesson| shown_line(&lesson.fields)).collect();
    let numbers = found.iter().map(|lesson| lesson.id).collect();
    Ok((render(spec.unwrap_or_default(), "lessons", &events, &[], &[]), numbers))
}

/// A linha como a leitura mostra, com o código do item (`MSTD-<sigla>-<NNNN>`),
/// que é o jeito de citá-lo e o endereço dele na página: o gravado na linha
/// ou, numa linha sem código, o que a leitura dá a ela. `shown` diz o que sai
/// dela: o envio sem o pedido (`text`) na lista das ondas e no painel, e no
/// painel sem termo também a injeção e a entrega sem o `text`.
fn shown_with_code(event: &SpecEvent, codes: &BTreeMap<u64, String>, shown: Shown) -> String {
    let mut fields = event.fields.clone();
    if let Some(code) = codes.get(&event.id) {
        fields.insert("code".into(), Value::String(code.clone()));
    }
    let kind = event.event_type.as_str();
    let long_text = match shown {
        Shown::Whole | Shown::Agent => false,
        Shown::Plan => kind == "send",
        Shown::Panel => PANEL_WITHOUT_TEXT.contains(&kind),
    };
    if long_text {
        fields.remove("text");
    }
    if shown == Shown::Agent {
        without_agent_omissions(&mut fields, kind);
    }
    shown_line(&fields)
}

/// O que a leitura mostra de cada evento.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shown {
    /// A linha inteira.
    Whole,
    /// A lista das ondas: o envio sai sem o pedido.
    Plan,
    /// O painel sem termo: o envio, a injeção e a entrega saem sem o texto.
    Panel,
    /// A linha de quem lê da cópia de um pedido aberto: sem as marcas do
    /// binário e sem as palavras de busca.
    Agent,
}

/// Os tipos que o painel sem termo mostra sem o `text`.
const PANEL_WITHOUT_TEXT: &[&str] = &["send", "injection", "delivered"];

/// Os campos que quem lê da cópia de um pedido não lê: as marcas que o binário
/// carimba (`v`, `at`, `author`) e as palavras de busca (`keys`).
const AGENT_OMITTED: &[&str] = &["v", "at", "author", "keys"];

/// Tira da linha, e do `changed` dela, o que quem lê da cópia de um pedido não
/// lê ([`AGENT_OMITTED`]). A mensagem do usuário guarda o `at` e o `author`:
/// dizem quem falou e quando.
fn without_agent_omissions(fields: &mut Map<String, Value>, event_type: &str) {
    let kept: &[&str] = if event_type == "message" { &["at", "author"] } else { &[] };
    for key in AGENT_OMITTED.iter().filter(|key| !kept.contains(key)) {
        fields.remove(*key);
        if let Some(Value::Object(changed)) = fields.get_mut("changed") {
            changed.remove(*key);
        }
    }
    if fields.get("changed").and_then(Value::as_object).is_some_and(Map::is_empty) {
        fields.remove("changed");
    }
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

/// Roda o comando `mustard-rt run read <bloco> --root <raiz> --spec <spec>`
/// que uma resposta do Mustard entrega no lugar do texto, pela mesma leitura
/// do comando, e devolve o que ele imprime. O texto que não é esse comando
/// derruba o teste. Os testes leem por aqui como o agente lê.
#[cfg(test)]
pub(crate) fn read_by_command(command: &str) -> String {
    let words: Vec<&str> = command.split_whitespace().collect();
    assert_eq!(words.get(..3), Some(&["mustard-rt", "run", "read"][..]), "{command}");
    let flag = |name: &str| words.iter().position(|w| *w == name).and_then(|i| words.get(i + 1)).map(|w| (*w).to_string());
    let opts = ReadOpts {
        root: PathBuf::from(flag("--root").unwrap_or_else(|| panic!("no --root: {command}"))),
        spec: flag("--spec"),
        block: words[3].to_string(),
        term: None,
    };
    read_at(&opts).unwrap_or_else(|refusal| panic!("{command}: {refusal}"))
}

/// Run `read` and print the block; exit 1 on a refusal. The recorded request
/// of a wave, or of the final review, prints byte for byte, with no newline
/// added.
pub fn run(opts: &ReadOpts) {
    match read_at(opts) {
        Ok(text) if matches!(ReadQuery::parse(&opts.block), Some(ReadQuery::Request(_) | ReadQuery::ReviewRequest)) => {
            print!("{text}");
        }
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
        let c1 = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "echo p", "form": "ubiquitous", "origin": said}));
        let c2 = put(root, "criterion", json!({"when": "c", "then": "d", "proof": "echo q", "form": "ubiquitous", "origin": said}));
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
        put(root, "criterion", json!({"when": "a", "then": "b", "proof": "echo p", "keys": ["C-1"], "form": "ubiquitous", "origin": said}));
        put(root, "criterion", json!({"when": "c", "then": "d", "proof": "echo q", "keys": ["C-2"], "form": "ubiquitous", "origin": said}));
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

        let report = read_for(&without_spec(root, "conversation"), Some("s-leitura"), root).unwrap();
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

        let report = read_for(&without_spec(root, "conversation"), Some("s-leitura"), root).unwrap();
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
        let pt = read_for(&without_spec(root, "state"), None, root).unwrap_err();
        assert_eq!(pt["ok"], json!(false));
        assert_eq!(pt["reason"], json!("no-current-spec"));
        assert!(pt["hint"].as_str().unwrap().starts_with("Nenhuma spec atual"), "{pt}");

        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"en-US"}}"#).unwrap();
        let en = read_for(&without_spec(root, "state"), None, root).unwrap_err();
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
            let c = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "echo p", "form": "ubiquitous", "origin": said}));
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

    /// O registro que toma o lugar de uma linha cortada pelo disco cheio se lê
    /// pelo número e pelo código da linha que ele substituiu, sem aviso de
    /// linha pulada, e nenhuma outra leitura o mostra como item de trabalho:
    /// nem os blocos, nem o pedido de despacho, nem o backlog.
    #[test]
    fn a_cut_line_record_reads_by_number_and_never_shows_as_a_work_item() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (_, _, _, _) = plan_with_a_new_version(root);
        let next_id = put(root, "message", json!({"author": "user", "text": "outro"}));
        let path = store::spec_file(root, "teste").expect("spec file");
        let piece = r#"{"v":1,"id":10,"code":"MSTD-RULE-0009","at":"2026-09-11T09:03:00-03:00","type":"rule","text":"Regra do meio","keys":["meio"],"exa"#;
        let mut raw = std::fs::read_to_string(&path).unwrap();
        raw.push_str(&format!("{piece}\n"));
        std::fs::write(&path, raw).unwrap();
        assert_eq!(next_id, 9);

        let written = put(root, "message", json!({"author": "user", "text": "depois"}));
        assert_eq!(written, 11, "the cut line's number is kept");

        let (report, item) = only_line(root, "item-10");
        assert!(!report.contains("warnings"), "{report}");
        assert_eq!(item["type"], json!("cut_line"), "{report}");
        assert_eq!(item["piece"], json!(piece), "{report}");
        assert_eq!(item["code"], json!("MSTD-RULE-0009"), "{report}");
        let (_, by_code) = only_line(root, "item-MSTD-RULE-0009");
        assert_eq!(by_code, item, "the code of the lost line finds the record");

        let mut blocks: Vec<String> = Block::ALL.iter().map(|b| b.name().to_string()).collect();
        blocks.extend(["dispatch-1", "dispatch-2", "backlog", "wave-1", "wave-2"].map(str::to_string));
        for block in blocks {
            let report = read_at(&opts(root, &block, None)).unwrap();
            assert!(!report.contains("cut_line") && !report.contains("Regra do meio"), "{block}: {report}");
            assert!(!report.contains("warnings"), "{block}: {report}");
        }
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

    /// A mensagem do usuário lida como item sai com o texto inteiro, o autor
    /// e o código, como qualquer outro item: o agente que a tarefa atende lê o
    /// que o usuário disse, pelo código ou pelo número.
    #[test]
    fn a_message_read_as_an_item_shows_its_whole_text_and_code() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "Pode liberar mais espaço, é voce que está lotando o disco"}));

        for block in [format!("item-{said}"), "item-MSTD-MSG-0001".to_string()] {
            let (report, item) = only_line(root, &block);
            assert_eq!(item["id"], json!(said), "{block}: {report}");
            assert_eq!(item["code"], json!("MSTD-MSG-0001"), "{block}: {report}");
            assert_eq!(item["author"], json!("user"), "{block}: {report}");
            assert_eq!(
                item["text"],
                json!("Pode liberar mais espaço, é voce que está lotando o disco"),
                "{block}: {report}"
            );
        }
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
            let c = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "echo p", "form": "ubiquitous", "origin": said}));
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

    /// A lista das ondas traz uma linha curta por onda, na ordem do número
    /// e não na do arquivo: o código, o número, a primeira linha não vazia do
    /// texto e as ondas de que ela depende. A versão substituída e a onda
    /// removida ficam de fora, e a tarefa, o pedido e a entrega não entram.
    #[test]
    fn the_wave_list_brings_one_short_line_per_wave_in_number_order() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "o plano"}));
        let c = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "echo p", "form": "ubiquitous", "origin": said}));
        let wave = |n: u64, text: &str, depends_on: &[u64], replaces: Option<u64>| -> u64 {
            let mut body = json!({"n": n, "text": text, "criteria": [c], "done_when": "x", "depends_on": depends_on, "origin": said});
            if let Some(old) = replaces {
                body["replaces"] = json!(old);
            }
            put(root, "wave", body)
        };
        let old_first = wave(1, "Somar.", &[], None);
        let third = wave(3, "Mostrar o total.\nCom o detalhe de cada parcela.", &[1, 2], None);
        let second = wave(2, "\n  Guardar a soma.  \n\nE o resto.", &[1], None);
        let gone = wave(4, "Sair.", &[], None);
        let first = wave(1, "Somar as parcelas.", &[], Some(old_first));
        put(root, "task", json!({"wave": 1, "text": "T1.", "files": [{"path": "a.rs"}], "depends_on": [], "origin": said}));
        put(root, "remove", json!({"targets": [gone], "reason": "Saiu do plano."}));
        by_program(root, "send", json!({"author": "binary", "wave": 1, "role": "wave", "text": "O pedido inteiro da onda.",
            "lines": 1, "chars": 25, "mustard": "0.2.0"}));
        by_program(root, "delivered", json!({"author": "binary", "wave": 1, "text": "Pronta.", "files": ["a.rs"]}));

        let report = read_at(&opts(root, "wave-list", None)).unwrap();
        let expected = format!(
            "{{\"ok\":true,\"spec\":\"teste\",\"block\":\"wave-list\",\"count\":3,\"events\":[\n\
             {{\"id\":{first},\"code\":\"MSTD-WAVE-0001\",\"depends_on\":[],\"first_line\":\"Somar as parcelas.\",\"n\":1}},\n\
             {{\"id\":{second},\"code\":\"MSTD-WAVE-0003\",\"depends_on\":[1],\"first_line\":\"Guardar a soma.\",\"n\":2}},\n\
             {{\"id\":{third},\"code\":\"MSTD-WAVE-0002\",\"depends_on\":[1,2],\"first_line\":\"Mostrar o total.\",\"n\":3}}\n\
             ]}}"
        );
        assert_eq!(report, expected);
    }

    /// Uma chamada gravada como o programa a grava, com o autor e o comando.
    fn call(root: &std::path::Path, command: &str, fields: Value) -> u64 {
        let mut body = json!({"author": "binary", "command": command, "result": "ok"});
        if let (Some(body), Some(fields)) = (body.as_object_mut(), fields.as_object()) {
            body.extend(fields.clone());
        }
        by_program(root, "call", body)
    }

    /// O nome do comando acha as chamadas dele, na ordem do arquivo, sem olhar
    /// maiúsculas nem espaços de sobra; a busca por nota, que acharia a
    /// entrega que cita a busca, não entra, e a chamada de outro comando fica
    /// de fora.
    #[test]
    fn the_name_of_a_command_finds_its_calls() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let first = call(root, "map search", json!({"ms": 1800}));
        call(root, "round", json!({"ms": 3}));
        by_program(root, "delivered", json!({"author": "binary", "wave": 1, "text": "A map search ficou pronta.", "files": []}));
        let second = call(root, "map search", json!({"ms": 900}));

        let ids = |block: &str, term: &str| -> Vec<Value> {
            events(&read_at(&opts(root, block, Some(term))).unwrap()).iter().map(|e| e["id"].clone()).collect()
        };
        assert_eq!(ids("metrics", "map search"), [json!(first), json!(second)]);
        assert_eq!(ids("conversation", " Map  Search "), [json!(first), json!(second)]);
        assert_eq!(ids("metrics", "pronta").len(), 1, "a word that names no command still searches by score");
    }

    /// A soma das chamadas traz, por comando, quantas foram, as falhas por
    /// motivo, a mediana, o p90 e o pior do tempo da chamada e do tempo do
    /// filtro pela posição mais próxima, sem média entre dois valores, e os
    /// tokens e o custo somados; o comando sem filtro fica sem os campos dele,
    /// e o nome do comando deixa só a linha dele.
    #[test]
    fn the_calls_reading_sums_each_command() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let jev = |ms: u64, filter_ms: u64, tokens: u64, cost: u64| {
            json!({"ms": ms, "filter": "jev", "filter_ms": filter_ms, "tokens": tokens, "cost_micro_usd": cost})
        };
        call(root, "map search", jev(100, 90, 20_000, 840));
        call(root, "map search", jev(900, 800, 21_000, 882));
        call(root, "map search", json!({"ms": 300, "filter": "jev:busy", "filter_ms": 40}));
        call(root, "map search", jev(700, 600, 22_000, 924));
        call(root, "round", json!({"ms": 3}));
        call(root, "map search", jev(500, 400, 20_000, 840));
        call(root, "map search", json!({"ms": 200, "result": "refused", "refusal": "no-map"}));
        call(root, "map search", jev(800, 700, 21_000, 882));
        call(root, "round", json!({"ms": 7, "result": "refused"}));
        call(root, "map search", json!({"ms": 400, "filter": "jev:busy", "filter_ms": 60}));
        call(root, "map search", jev(1000, 950, 22_000, 924));
        call(root, "round", json!({"ms": 5}));
        call(root, "map search", jev(600, 500, 20_000, 840));

        let search = "{\"command\":\"map search\",\"cost_micro_usd\":6132,\"count\":10,\
            \"failures\":{\"jev:busy\":2,\"no-map\":1},\"filter_ms\":{\"max\":950,\"median\":500,\"p90\":950},\
            \"ms\":{\"max\":1000,\"median\":500,\"p90\":900},\"tokens\":146000}";
        let round = "{\"command\":\"round\",\"count\":3,\"failures\":{\"refused\":1},\"ms\":{\"max\":7,\"median\":5,\"p90\":7}}";
        let envelope = |count: usize, lines: &str| {
            format!("{{\"ok\":true,\"spec\":\"teste\",\"block\":\"calls\",\"count\":{count},\"events\":[\n{lines}\n]}}")
        };
        assert_eq!(read_at(&opts(root, "calls", None)).unwrap(), envelope(2, &format!("{search},\n{round}")));
        assert_eq!(read_at(&opts(root, "calls", Some("map search"))).unwrap(), envelope(1, search));
        assert!(events(&read_at(&opts(root, "calls", Some("close"))).unwrap()).is_empty());
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

    /// O estado traz só a cópia mais nova de cada página, em vez de uma cópia
    /// por rodada: com sete cópias da página da spec e três da página do
    /// projeto, a leitura mostra duas, cada uma a última da sua página, e os
    /// outros eventos do estado seguem todos.
    #[test]
    fn the_state_block_carries_only_the_newest_copy_of_each_page() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let opened = by_program(root, "state", json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
        let mut spec_copies = Vec::new();
        for last in 1..=7 {
            spec_copies.push(by_program(root, "copy", json!({"author": "binary", "page": "spec", "last": last * 10})));
            if last % 3 == 0 {
                by_program(root, "copy", json!({"author": "binary", "page": "project", "phase": "plan"}));
            }
        }
        let running = by_program(root, "state", json!({"author": "binary", "phase": "running"}));

        let report = read_at(&opts(root, "state", None)).unwrap();
        let got = events(&report);
        let kinds: Vec<(&str, u64)> =
            got.iter().map(|e| (e["type"].as_str().unwrap(), e["id"].as_u64().unwrap())).collect();
        let copies: Vec<&Value> = got.iter().filter(|e| e["type"] == json!("copy")).collect();
        assert_eq!(copies.len(), 2, "one copy per page: {report}");
        assert_eq!(
            copies.iter().find(|e| e["page"] == json!("spec")).map(|e| e["id"].as_u64().unwrap()),
            spec_copies.last().copied(),
            "the spec page keeps its newest copy: {report}"
        );
        assert!(copies.iter().any(|e| e["page"] == json!("project")), "the project page keeps a copy: {report}");
        assert!(kinds.contains(&("state", opened)) && kinds.contains(&("state", running)), "{report}");
        assert_eq!(got.len(), 4, "{report}");
    }

    /// O plano dos testes do registro: uma onda com uma tarefa e uma regra,
    /// e o envio dela com a vaga `copy` (a pasta `copias/a` dentro do
    /// projeto, com uma subpasta de código). Devolve a vaga, o código da regra
    /// e o número do envio.
    fn open_request(root: &std::path::Path) -> (PathBuf, u64) {
        let said = put(root, "message", json!({"author": "user", "text": "o pedido"}));
        put(root, "rule", json!({"title": "Regra", "text": "Vale sempre.", "agent": "detalhe", "keys": ["regra"], "example": "e", "origin": said}));
        let c = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "echo p", "form": "ubiquitous", "origin": said}));
        put(root, "wave", json!({"n": 1, "text": "Onda.", "criteria": [c], "done_when": "x", "origin": said}));
        put(root, "task", json!({"wave": 1, "text": "Fazer.", "files": [{"path": "a.rs"}], "depends_on": [], "origin": said}));
        let copy = root.join("copias").join("a");
        std::fs::create_dir_all(copy.join("apps").join("rt")).unwrap();
        let sent = by_program(root, "send", json!({"author": "binary", "wave": 1, "role": "wave", "text": "pedido", "lines": 1,
            "chars": 6, "mustard": "0", "copy": mustard_core::io::wave_prompt::shown(&copy)}));
        (copy, sent)
    }

    /// As leituras registradas, na ordem do arquivo: o pedido e o item de
    /// cada chamada `read`.
    fn recorded_reads(root: &std::path::Path) -> Vec<(String, String)> {
        let path = store::spec_file(root, "teste").expect("spec file");
        let log = store::read(&path).unwrap().unwrap();
        log.visible()
            .into_iter()
            .filter(|e| e.event_type == "call" && e.str_field("command") == Some("read"))
            .map(|e| {
                assert_eq!(e.str_field("author"), Some("binary"));
                (e.str_field("request").unwrap_or_default().to_string(), e.str_field("item").unwrap_or_default().to_string())
            })
            .collect()
    }

    fn read_from(root: &std::path::Path, from: &std::path::Path, block: &str, term: Option<&str>) -> String {
        read_for(&opts(root, block, term), None, from).unwrap()
    }

    /// Ler um item de dentro da vaga do envio aberto, ou de uma pasta dentro
    /// dela, grava a leitura com o pedido e o item; ler o mesmo item de
    /// outra pasta não grava, nem ler um código que não existe.
    #[test]
    fn an_item_read_from_inside_the_copy_of_an_open_request_is_recorded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);

        read_from(root, &copy, "item-MSTD-RULE-0001", None);
        read_from(root, &copy.join("apps").join("rt"), "item-MSTD-TASK-0001", None);
        assert_eq!(
            recorded_reads(root),
            [("request-1".into(), "MSTD-RULE-0001".into()), ("request-1".into(), "MSTD-TASK-0001".into())]
        );

        read_from(root, root, "item-MSTD-RULE-0001", None);
        read_from(root, &copy.parent().unwrap().join("outra"), "item-MSTD-RULE-0001", None);
        read_from(root, &copy, "item-MSTD-RULE-0099", None);
        assert_eq!(recorded_reads(root).len(), 2, "outra pasta e código que não existe não gravam");
    }

    /// Pelo número, o item grava o código dele; a lição grava
    /// `lesson-<número>` e só quando o banco a tem.
    #[test]
    fn a_read_by_number_records_the_code_and_a_lesson_records_its_number() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);
        let rule = put(root, "rule", json!({"title": "Outra", "text": "Vale também.", "agent": "d", "keys": ["outra"], "example": "e", "origin": 1}));
        let bank = mustard_core::ClaudePaths::for_project(root).unwrap().lessons_path();
        let lesson = write_lesson(&bank, "Nunca comitar sem rodar a suíte inteira.", &["suíte"]);

        read_from(root, &copy, &format!("item-{rule}"), None);
        read_from(root, &copy, "lessons", Some(&lesson.to_string()));
        read_from(root, &copy, "lessons", Some("999"));
        assert_eq!(
            recorded_reads(root),
            [("request-1".into(), "MSTD-RULE-0002".into()), ("request-1".into(), format!("lesson-{lesson}"))]
        );
    }

    /// Ler `dispatch-<n>` de dentro da cópia da onda `n` conta todos os
    /// itens e lições que ele lista; de dentro da cópia de outra onda, ou de
    /// outra pasta, não conta nada.
    #[test]
    fn the_dispatch_read_from_the_copy_of_its_wave_records_everything_it_lists() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);
        let report = read_from(root, root, "dispatch-1", None);
        let listed: Vec<String> =
            events(&report).iter().map(|e| e["code"].as_str().expect("every listed item has a code").to_string()).collect();
        assert!(listed.contains(&"MSTD-TASK-0001".to_string()) && listed.contains(&"MSTD-WAVE-0001".to_string()), "{listed:?}");
        assert!(recorded_reads(root).is_empty(), "lido de fora da cópia não grava");

        read_from(root, &copy, "dispatch-1", None);
        let recorded: Vec<String> = recorded_reads(root).into_iter().map(|(request, item)| {
            assert_eq!(request, "request-1");
            item
        }).collect();
        assert_eq!(recorded, listed);

        let c2 = put(root, "criterion", json!({"when": "c", "then": "d", "proof": "echo q", "form": "ubiquitous", "origin": 1}));
        put(root, "wave", json!({"n": 2, "text": "Dois.", "criteria": [c2], "done_when": "y", "origin": 1}));
        read_from(root, &copy, "dispatch-2", None);
        assert_eq!(recorded_reads(root).len(), listed.len(), "o pedido da onda 2 não se lê da cópia da onda 1");
    }

    /// O envio de revisão aberto grava `request-review`, e o pedido que já
    /// tem entrega ou veredito deixa de estar aberto e não grava mais.
    #[test]
    fn the_review_request_and_a_closed_request_are_told_apart() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);
        let review = root.join("copias").join("revisao");
        std::fs::create_dir_all(&review).unwrap();
        by_program(root, "send", json!({"author": "binary", "role": "review", "text": "revise", "lines": 1, "chars": 6,
            "mustard": "0", "copy": mustard_core::io::wave_prompt::shown(&review)}));

        read_from(root, &review, "item-MSTD-RULE-0001", None);
        assert_eq!(recorded_reads(root), [("request-review".into(), "MSTD-RULE-0001".into())]);

        by_program(root, "delivered", json!({"author": "binary", "wave": 1, "text": "Pronta.", "files": []}));
        read_from(root, &copy, "item-MSTD-RULE-0001", None);
        assert_eq!(recorded_reads(root).len(), 1, "a onda entregue não tem mais pedido aberto");
    }

    /// Os campos que a leitura de quem trabalha na cópia deixa de fora.
    const STAMPS: [&str; 4] = ["v", "at", "author", "keys"];

    /// A regra do plano de `open_request` ganha uma versão nova, com outro
    /// autor e outras palavras de busca, para o `changed` ter o que dizer.
    fn revise_the_rule(root: &std::path::Path) {
        let old = events(&read_from(root, root, "item-MSTD-RULE-0001", None))[0]["id"].as_u64().unwrap();
        by_program(root, "rule", json!({"author": "binary", "title": "Regra", "text": "Vale sempre, agora.", "agent": "detalhe",
            "keys": ["regra", "nova"], "example": "e", "origin": 1, "replaces": old}));
    }

    /// De dentro da cópia de um pedido aberto, o item vem sem as marcas do
    /// binário e sem as palavras de busca, e o `changed` sem o autor e sem as
    /// palavras; o texto, o título, o que é para o agente, o exemplo e os
    /// números que se seguem ficam. De fora da cópia, a linha vem inteira.
    #[test]
    fn an_item_read_from_inside_the_copy_comes_without_the_stamps_and_the_search_words() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);
        revise_the_rule(root);

        let whole = events(&read_from(root, root, "item-MSTD-RULE-0001", None))[0].clone();
        for key in STAMPS {
            assert!(whole.get(key).is_some(), "outside the copy the line keeps {key}: {whole}");
        }
        assert_eq!(whole["changed"]["author"], json!(true), "{whole}");
        assert!(whole["changed"]["keys"].is_object(), "{whole}");

        for from in [copy.clone(), copy.join("apps").join("rt")] {
            let seen = events(&read_from(root, &from, "item-MSTD-RULE-0001", None))[0].clone();
            for key in STAMPS {
                assert!(seen.get(key).is_none(), "inside the copy the line leaves out {key}: {seen}");
            }
            assert_eq!(seen["changed"], json!({"text": true}), "{seen}");
            for key in ["id", "code", "type", "title", "text", "agent", "example", "origin", "replaces"] {
                assert_eq!(seen[key], whole[key], "{key} stays: {seen}");
            }
        }
    }

    /// A mensagem do usuário guarda o `at` e o `author`, que dizem quem falou
    /// e quando, mesmo lida de dentro da cópia; as marcas do binário que não
    /// dizem nada dela saem.
    #[test]
    fn a_user_message_read_from_inside_the_copy_keeps_who_said_it_and_when() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);

        let message = events(&read_from(root, &copy, "item-1", None))[0].clone();
        assert_eq!(message["type"], json!("message"), "{message}");
        assert_eq!(message["author"], json!("user"), "{message}");
        assert!(message["at"].is_string(), "{message}");
        assert_eq!(message["text"], json!("o pedido"), "{message}");
        assert!(message.get("v").is_none(), "{message}");
    }

    /// A leitura do pedido inteiro, `dispatch-<n>`, vem sem as marcas nem as
    /// palavras de busca de dentro da cópia, em cada item que lista, e inteira
    /// de fora dela.
    #[test]
    fn the_dispatch_read_from_inside_the_copy_comes_without_the_stamps_and_the_search_words() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (copy, _) = open_request(root);
        put(root, "rule", json!({"title": "Global", "text": "Vale para tudo.", "agent": "d", "keys": ["global"], "example": "e", "origin": 1,
            "applies_to": {"files": ["**"]}}));

        let outside = events(&read_from(root, root, "dispatch-1", None));
        assert!(outside.iter().any(|e| e["type"] == json!("rule")), "the dispatch lists the global rule: {outside:?}");
        assert!(outside.iter().all(|e| e.get("v").is_some() && e.get("at").is_some()), "{outside:?}");
        assert!(outside.iter().any(|e| e.get("keys").is_some()), "{outside:?}");

        let inside = events(&read_from(root, &copy, "dispatch-1", None));
        assert_eq!(inside.len(), outside.len(), "{inside:?}");
        for event in &inside {
            // A mensagem do usuário guarda o `at` e o `author`.
            let kept: &[&str] = if event["type"] == json!("message") { &["at", "author"] } else { &[] };
            for key in STAMPS.iter().filter(|key| !kept.contains(key)) {
                assert!(event.get(*key).is_none(), "{key} left out of {event}");
            }
            assert!(event["code"].is_string() && event["type"].is_string(), "{event}");
        }
        assert!(inside.iter().any(|e| e["type"] == json!("message") && e["author"] == json!("user")), "{inside:?}");
    }

    /// O painel sem termo vem sem o texto do envio, da injeção e da entrega,
    /// que eram mais da metade da saída; o tamanho, o gancho, o bloqueio e o
    /// tempo seguem. Com um termo, o texto vem; o código do item traz o texto
    /// inteiro; e a lista das ondas segue com o texto da entrega.
    #[test]
    fn the_panel_without_a_term_leaves_out_the_long_text_of_injections_and_deliveries() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let injection = by_program(root, "injection", json!({"author": "hook", "hook": "prompt_entry", "chars": 23, "text": "Texto longo da injecao."}));
        by_program(root, "hook", json!({"author": "hook", "hook": "write_gate", "tool": "Write", "action": "block", "reason": "o motivo do bloqueio"}));
        call(root, "round", json!({"ms": 3}));
        by_program(root, "send", json!({"author": "binary", "wave": 1, "role": "wave", "text": "o pedido", "lines": 1, "chars": 8, "mustard": "0", "copy": "x"}));
        by_program(root, "delivered", json!({"author": "binary", "wave": 1, "text": "O relato da entrega.", "files": ["a.rs"]}));

        let panel = events(&read_at(&opts(root, "metrics", None)).unwrap());
        let of = |kind: &str| panel.iter().find(|e| e["type"] == json!(kind)).unwrap_or_else(|| panic!("no {kind}: {panel:?}")).clone();
        for kind in ["injection", "delivered", "send"] {
            assert!(of(kind).get("text").is_none(), "the panel leaves out the text of {kind}: {}", of(kind));
        }
        assert_eq!((of("injection")["chars"].clone(), of("injection")["hook"].clone()), (json!(23), json!("prompt_entry")));
        assert_eq!(of("delivered")["files"], json!(["a.rs"]));
        assert_eq!(of("hook")["reason"], json!("o motivo do bloqueio"));
        assert_eq!(of("call")["ms"], json!(3));

        let found = events(&read_at(&opts(root, "metrics", Some("longo"))).unwrap());
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0]["text"], json!("Texto longo da injecao."), "a term asks for the content");

        let by_code = events(&read_at(&opts(root, &format!("item-{}", of("injection")["code"].as_str().unwrap()), None)).unwrap());
        assert_eq!(by_code[0]["id"], json!(injection));
        assert_eq!(by_code[0]["text"], json!("Texto longo da injecao."));

        let waves = events(&read_at(&opts(root, "waves", None)).unwrap());
        let delivered = waves.iter().find(|e| e["type"] == json!("delivered")).expect("the waves list the delivery");
        assert_eq!(delivered["text"], json!("O relato da entrega."), "the plan keeps the delivery text");
        let send = waves.iter().find(|e| e["type"] == json!("send")).expect("the waves list the send");
        assert!(send.get("text").is_none(), "{send}");
    }
}
