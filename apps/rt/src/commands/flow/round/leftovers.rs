//! As sobras de uma volta: o que o agente de onda viu e deixou de fora da
//! entrega, no campo `leftovers`. Toda sobra vai ao backlog da spec, sem
//! pergunta: o que nasce na obra se resolve nela, e a rodada nunca passa um
//! achado à lista de pendências do projeto. A sobra vira um pedido próprio,
//! com o título e o relato dela, e uma tarefa que cobre esse pedido — nunca
//! os critérios da onda que a relatou, que quase nunca têm a ver com ela.
//! Todo achado diz a regra que quebrou (`rule`) e os lugares dela
//! (`places`), e se junta pela regra, sem classificador: entra na tarefa da
//! mesma regra — a que outro achado da volta já trouxe, ou a aberta do
//! backlog que faz valer a regra, que a guarda em `rule` — e, sem nenhuma,
//! vira a tarefa nova da regra. Regra diferente é tarefa diferente, mesmo no
//! mesmo arquivo. O achado que entra numa tarefa que já existe dá o pedido
//! com a marca da sobra no título, e o pedido da onda dessa tarefa a mostra
//! no que ela atende. A tarefa do achado sai com a prioridade dele, porque
//! erro achado na obra vai à frente da fila. Os arquivos do achado são os que
//! ele declara em `files`; sem a lista, os que o detalhe cita. O achado sem
//! arquivo nenhum vira tarefa presa, a planejar: ela não sai numa onda até
//! quem conduz a obra gravar uma versão nova com os arquivos e as
//! dependências. O achado que o agente marca como limpeza (`cleanup: true`)
//! vira tarefa com a mesma marca, sem prioridade, e sai na onda do mesmo
//! sentido, como as outras tarefas do backlog. O campo `kind` que uma volta
//! antiga ainda traga é aceito e ignorado.
//!
//! Os achados que a revisão final aprovada traz em `leftovers`, no mesmo
//! formato, passam pela mesma montagem quando o fechamento os grava
//! ([`record_final_leftovers`]), com os textos que nomeiam a revisão final no
//! lugar da onda.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::project_map::cited_paths;
use mustard_core::domain::spec_events::{Kind, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::text;
use mustard_core::io::wave_prompt;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use mustard_core::domain::spec_state::PhaseWriter;

use super::git_ignored::all_ignored;
use super::jev_decision::FINAL_REVIEW;
use super::queue::backlog_left;
use super::rehearsal::{Recording, Rehearsed, rehearse};
use super::report::{backlog_return, own_copy_relative};
use super::stops::append_agent_line;
use crate::commands::spec_events::write::RecordCheck;
use crate::shared::task_triage;

/// O campo que marca a limpeza, na sobra da volta e na tarefa que ela vira:
/// o nome mora só aqui.
const CLEANUP: &str = "cleanup";

/// O campo da sobra com os arquivos que ela pede para mudar.
const FILES: &str = "files";

/// O campo da prioridade da tarefa.
const PRIORITY: &str = "priority";

/// O campo da regra que o achado quebrou, e da regra que a tarefa nascida
/// dele faz valer: é por ele que os achados se juntam numa tarefa só.
const RULE: &str = "rule";

/// O campo dos lugares da regra que o achado diz, e dos que a tarefa nascida
/// dele guarda, para a entrega dela mudar cada um.
const PLACES: &str = "places";

/// O campo do grupo de assunto da tarefa: o nome do grupo e a posição dela
/// nele. A montagem lê o grupo pelo mesmo nome ([`group_name`]).
pub(super) const GROUP: &str = "group";

/// Um texto que nomeia quem apontou a sobra: a chave do texto da onda, que
/// leva o número dela em `{wave}`, e a do texto da revisão final, que não tem
/// onda ([`said_by`]).
type SaidBy = (&'static str, &'static str);

/// A seção do achado que entra na tarefa da mesma regra.
const JOINED_BY_RULE: SaidBy = ("finding.joined_by_rule", "finding.review_joined_by_rule");

/// A prioridade da tarefa que a sobra grava ([`leftover_priority`]).
const PRIORITY_SAID: SaidBy = ("round.leftover_priority", "round.leftover_review_priority");

/// A marca da sobra no título do pedido dela ([`marked_title`]).
const MARKED: SaidBy = ("round.leftover_marked", "round.leftover_review_marked");

/// A nota da sobra só em arquivo que o git ignora ([`local_note`]).
const LOCAL: SaidBy = ("round.leftover_local", "round.leftover_review_local");

/// O tipo do evento que a sobra vira quando é trabalho: a tarefa nova ou a
/// versão nova de uma tarefa aberta.
const TASK: &str = "task";

/// O tipo do evento que a sobra vira quando é só mudança local
/// ([`local_note`]).
const NOTE: &str = "note";

/// O tipo do evento que guarda o relato da sobra, e que a tarefa cobre.
const REQUEST: &str = "request";

/// Uma sobra da volta: o título e o detalhe dela, os arquivos que ela
/// declara mudar (`None` quando a volta não traz a lista), e se o agente a
/// marcou como limpeza — só muda comentário, documentação ou texto de ajuda,
/// sem mudar comportamento nem o que um teste espera. O achado diz também a
/// regra que quebrou, pelo código de uma regra da spec ou pelo texto de uma
/// regra nova, e os lugares dessa regra.
pub(crate) struct Leftover {
    pub title: String,
    pub detail: String,
    pub files: Option<Vec<String>>,
    pub cleanup: bool,
    pub rule: String,
    pub places: Vec<String>,
}

/// As sobras da lista `leftovers` da volta, um evento `event_type` — a
/// entrega da onda ou o veredito final —, na ordem, cada texto sem os
/// espaços das pontas. O `kind` que ela ainda traga não é lido, e a marca de
/// limpeza ausente vale "não". O achado sem título, detalhe, regra ou lugar
/// nenhum é recusado pelo número dele, a contar de um, como a marca de
/// limpeza que não é sim nem não, a lista de arquivos ou de lugares que não é
/// uma lista de textos e a regra que não é texto.
///
/// # Errors
///
/// [`Refusal::LeftoverFieldMissing`] quando falta ao achado o título, o
/// detalhe, a regra ou os lugares; [`Refusal::InvalidValue`] quando a marca de
/// limpeza dele não é um booleano, a regra dele não é texto, ou os arquivos
/// ou os lugares dele não são uma lista de textos.
pub(super) fn leftovers_of(event_type: &str, items: &[Value]) -> Result<Vec<Leftover>, Refusal> {
    let field = |item: &Value, key: &str| item.get(key).and_then(Value::as_str).map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    let invalid = |at: usize, name: &str, expected: Kind| Refusal::InvalidValue {
        event_type: event_type.into(),
        field: format!("leftovers[{}].{name}", at + 1),
        expected,
    };
    let texts = |at: usize, item: &Value, name: &str| match item.get(name) {
        None => Ok(None),
        Some(Value::Array(listed)) => listed
            .iter()
            .map(|said| said.as_str().map(|said| said.trim().to_string()))
            .collect::<Option<Vec<_>>>()
            .map(Some)
            .ok_or_else(|| invalid(at, name, Kind::Texts)),
        Some(_) => Err(invalid(at, name, Kind::Texts)),
    };
    let mut leftovers = Vec::new();
    for (at, item) in items.iter().enumerate() {
        let cleanup = match item.get(CLEANUP) {
            None => false,
            Some(Value::Bool(marked)) => *marked,
            Some(_) => return Err(invalid(at, CLEANUP, Kind::Bool)),
        };
        let files = texts(at, item, FILES)?;
        let places: Vec<String> = texts(at, item, PLACES)?.unwrap_or_default().into_iter().filter(|place| !place.is_empty()).collect();
        if item.get(RULE).is_some_and(|said| !said.is_null() && !said.is_string()) {
            return Err(invalid(at, RULE, Kind::Text));
        }
        let (title, detail, rule) = (field(item, "title"), field(item, "detail"), field(item, RULE));
        let missing: Vec<&str> = [("title", title.is_none()), ("detail", detail.is_none()), (RULE, rule.is_none()), (PLACES, places.is_empty())]
            .into_iter()
            .filter_map(|(name, absent)| absent.then_some(name))
            .collect();
        let (Some(title), Some(detail), Some(rule), false) = (title, detail, rule, places.is_empty()) else {
            return Err(Refusal::LeftoverFieldMissing { finding: at + 1, field: missing.join(", ") });
        };
        leftovers.push(Leftover { title, detail, files, cleanup, rule, places });
    }
    Ok(leftovers)
}

/// Um evento que os achados da volta viram, com a onda que o apontou e o tipo
/// dele. `asks` são os pedidos que a tarefa passa a cobrir, pela posição de
/// cada um na mesma lista: o número de um pedido só existe depois que ele é
/// gravado, e todos vêm antes das tarefas.
pub(super) struct LeftoverEvent {
    pub(super) wave: u64,
    pub(super) kind: &'static str,
    pub(super) draft: Map<String, Value>,
    pub(super) asks: Vec<usize>,
}

/// Uma tarefa que ganhou versão nova porque o classificador juntou a ela
/// itens não cumpridos de mesmo sentido: a posição da versão entre as
/// entradas que a rodada grava e o número de cada item, que já existe na
/// spec.
pub(super) struct MeaningJoin {
    pub(super) task: usize,
    pub(super) items: Vec<u64>,
}

/// Leva à entrada gravada de cada tarefa de `joins` o pedido de reescrita ao
/// agente de análise da própria sessão ([`task_triage::rewrite_request`]),
/// em `rewrite`, como a gravação e a reorganização da lista já fazem. A
/// versão nova, lida em `log`, é a tarefa que recebe, e cada item não
/// cumprido juntado a ela, o que entra nela. `entries` são as entradas que a
/// rodada gravou, nas posições de `joins`, cada uma com o número do evento em
/// `id`.
pub(super) fn ask_rewrites(root: &Path, log: &SpecLog, joins: &[MeaningJoin], entries: &mut [Value]) {
    let codes = log.codes();
    let source = |id: u64| log.get(id).map(|event| task_triage::rewrite_source(codes.get(&id).cloned().unwrap_or_else(|| id.to_string()), &event.fields));
    for join in joins {
        let Some(version) = entries.get(join.task).and_then(|entry| entry["id"].as_u64()) else { continue };
        let sources = std::iter::once(version).chain(join.items.iter().copied()).filter_map(source).collect();
        let request = task_triage::rewrite_request(root, version, sources);
        if let Some(entry) = entries.get_mut(join.task) {
            entry["rewrite"] = request;
        }
    }
}

/// Os eventos que os achados `found` viram, cada um com a onda que o apontou:
/// primeiro o pedido de cada achado ([`leftover_request`]), na ordem dos
/// achados, e depois as tarefas e as notas. O achado entra, com uma linha e o
/// pedido dele, na tarefa que outro achado da mesma regra desta volta trouxe
/// ou na aberta do backlog que guarda a mesma regra em `rule`, lida em `log`,
/// numa versão nova; sem nenhuma, vira a tarefa nova da regra
/// ([`leftover_task`]), com a regra e os lugares na parte do agente. A regra
/// se compara pelas palavras ([`comparable`]). O pedido do achado que entra
/// numa tarefa que já existe leva a marca da sobra no título
/// ([`marked_title`]), e o pedido aponta em `origin` a entrega da onda em
/// `delivered`, quando ela está lá. O achado só em arquivo que o git ignora
/// vira nota ao usuário, sem pedido nem tarefa ([`local_note`]). Os textos
/// saem no idioma `lang`.
pub(super) fn leftover_events(
    root: &Path,
    log: &SpecLog,
    found: &[(u64, &Leftover)],
    delivered: &BTreeMap<u64, u64>,
    lang: Locale,
) -> Vec<LeftoverEvent> {
    // As tarefas abertas do backlog que fazem valer uma regra, pela forma em
    // que a regra se compara.
    let of_rule: Vec<(String, &SpecEvent)> =
        backlog_left(log).into_iter().filter_map(|id| log.get(id)).filter_map(|task| Some((comparable(task.str_field(RULE)?), task))).collect();
    let mut requests: Vec<LeftoverEvent> = Vec::new();
    let mut work: Vec<LeftoverEvent> = Vec::new();
    // Onde está, em `work`, a tarefa de cada regra que um achado desta volta
    // já trouxe.
    let mut ruled: Vec<(String, usize)> = Vec::new();
    for &(wave, leftover) in found {
        let files: BTreeSet<String> = leftover_files(root, log, wave, leftover).into_iter().collect();
        // Só em arquivo que o git ignora é mudança local: nenhuma onda a
        // entrega, e o usuário a recebe como nota, sem pedido nem tarefa.
        if all_ignored(root, &files) {
            work.push(event(wave, NOTE, local_note(wave, leftover, &files, lang)));
            continue;
        }
        let key = comparable(&leftover.rule);
        let request = requests.len();
        requests.push(event(wave, REQUEST, leftover_request(leftover, &files, delivered.get(&wave).copied())));
        let earlier = ruled.iter().find(|(own, _)| *own == key).map(|(_, at)| *at);
        let at = earlier.or_else(|| {
            of_rule.iter().find(|(own, _)| *own == key).map(|(_, task)| {
                work.push(event(wave, TASK, backlog_return(task)));
                work.len() - 1
            })
        });
        let at = match at {
            Some(at) => {
                join(&mut work[at].draft, wave, leftover, &files, lang);
                work[at].asks.push(request);
                requests[request].draft.insert("title".into(), json!(marked_title(wave, &leftover.title, lang)));
                at
            }
            None => {
                let mut task = leftover_task(root, log, wave, leftover, lang);
                task.insert(RULE.into(), json!(leftover.rule));
                task.insert(PLACES.into(), json!(leftover.places));
                append_agent_line(&mut task, &rule_line(leftover, lang));
                work.push(LeftoverEvent { asks: vec![request], ..event(wave, TASK, task) });
                work.len() - 1
            }
        };
        if earlier.is_none() {
            ruled.push((key, at));
        }
    }
    requests.extend(work);
    requests
}

/// Grava no backlog da spec `spec`, vista de `start`, os achados que o
/// veredito final aprovado `verdict`, lido em `log`, traz em `leftovers`: cada
/// um vira o pedido com o relato dele, que aponta o veredito em `origin`, e a
/// tarefa que o cobre, pela mesma montagem das sobras de uma onda
/// ([`leftover_events`]), com a revisão final no lugar da onda
/// ([`FINAL_REVIEW`]). Tudo passa antes pelo ensaio: a recusa de um
/// não deixa gravado nenhum. Devolve, de cada evento gravado, o tipo e o
/// número; nada sem achado. Os textos saem no idioma `lang`.
///
/// # Errors
///
/// A recusa da leitura dos achados ([`leftovers_of`]), do ensaio ou da
/// gravação.
pub(crate) fn record_final_leftovers(start: &Path, root: &Path, spec: &str, (log, verdict): (&SpecLog, &SpecEvent), lang: Locale) -> Result<Vec<Value>, Refusal> {
    let listed = verdict.fields.get("leftovers").and_then(Value::as_array).cloned().unwrap_or_default();
    let found = leftovers_of("verdict", &listed)?;
    if found.is_empty() {
        return Ok(Vec::new());
    }
    let pointed: Vec<(u64, &Leftover)> = found.iter().map(|leftover| (FINAL_REVIEW, leftover)).collect();
    let origin = BTreeMap::from([(FINAL_REVIEW, verdict.id)]);
    let events = leftover_events(root, log, &pointed, &origin, lang);
    let mut check = RecordCheck::open(start, spec, PhaseWriter::Binary)?;
    let mut ids = Vec::new();
    let mut rehearsed: Vec<(&str, Rehearsed)> = Vec::new();
    for LeftoverEvent { kind, mut draft, asks, .. } in events {
        draft.insert("author".into(), json!("review"));
        cover_asks(&mut draft, &asks, &ids);
        let event = rehearse(&mut check, kind, draft)?;
        ids.push(event.id);
        rehearsed.push((kind, event));
    }
    let mut recording = Recording::new(start, spec);
    let mut recorded = Vec::new();
    for (kind, event) in rehearsed {
        let (id, _) = recording.record(kind, event)?;
        recorded.push(json!({ "type": kind, "id": id }));
    }
    Ok(recorded)
}

/// Liga a tarefa `draft` aos pedidos das sobras que ela passa a cobrir: cada
/// posição de `asks` aponta, em `ids`, o número que o ensaio deu ao pedido, e
/// ele entra em `covers`, sem repetir o que a tarefa já cobria.
pub(super) fn cover_asks(draft: &mut Map<String, Value>, asks: &[usize], ids: &[u64]) {
    if asks.is_empty() {
        return;
    }
    let mut covers: Vec<u64> = draft.get("covers").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect();
    for id in asks.iter().filter_map(|at| ids.get(*at).copied()) {
        if !covers.contains(&id) {
            covers.push(id);
        }
    }
    draft.insert("covers".into(), json!(covers));
}

/// O evento `kind` da sobra apontada pela onda `wave`, com o rascunho
/// `draft`, que ainda não cobre pedido nenhum.
fn event(wave: u64, kind: &'static str, draft: Map<String, Value>) -> LeftoverEvent {
    LeftoverEvent { wave, kind, draft, asks: Vec::new() }
}

/// A regra na forma em que dois achados, ou um achado e uma tarefa, se
/// comparam: as palavras dela, em minúsculas e sem acento, separadas por um
/// espaço só — a pontuação, a crase e a quebra de linha não contam.
fn comparable(said: &str) -> String {
    text::words(&text::fold(said)).collect::<Vec<_>>().join(" ")
}

/// A nota ao usuário da sobra `leftover`, apontada pela onda `wave`, cujos
/// arquivos `files` o git ignora todos: é mudança local, que nenhuma onda
/// entrega, e por isso não vira pedido nem tarefa. O título é o da sobra, o
/// texto diz a onda, os arquivos e o detalhe numa linha só, no idioma `lang`, e
/// os arquivos são as chaves de busca.
fn local_note(wave: u64, leftover: &Leftover, files: &BTreeSet<String>, lang: Locale) -> Map<String, Value> {
    let listed = files.iter().map(|file| format!("`{file}`")).collect::<Vec<_>>().join(", ");
    let text = said_by(wave, LOCAL, lang)
        .replace("{files}", &listed)
        .replace("{title}", &leftover.title)
        .replace("{detail}", &text::one_line(&leftover.detail));
    let note = json!({ "title": leftover.title, "text": text, "keys": files, "author": "wave" });
    note.as_object().cloned().unwrap_or_default()
}

/// O título do pedido da sobra de título `title`, apontada pela onda `wave`,
/// que entra numa tarefa que já existe: a marca da sobra antes do título, no
/// idioma `lang`. É ele que o pedido da onda da tarefa mostra no que ela
/// atende, e o agente vê que a tarefa ganhou esse trabalho.
fn marked_title(wave: u64, title: &str, lang: Locale) -> String {
    said_by(wave, MARKED, lang).replace("{title}", title)
}

/// O texto `said` da sobra apontada por `wave`, no idioma `lang`: o da onda,
/// com o número dela, ou, na sobra da revisão final ([`FINAL_REVIEW`]), o da
/// revisão — ela não tem onda, e o número zero não diria nada a quem lê.
fn said_by(wave: u64, (by_wave, by_review): SaidBy, lang: Locale) -> String {
    if wave == FINAL_REVIEW {
        translate(by_review, lang).to_string()
    } else {
        translate(by_wave, lang).replace("{wave}", &wave.to_string())
    }
}

/// O pedido que a sobra `leftover` vira: o título e o relato dela, para
/// ondas novas, com o título e os arquivos `files` como chaves de busca, e a
/// entrega `origin` que a relatou, quando se sabe qual.
fn leftover_request(leftover: &Leftover, files: &BTreeSet<String>, origin: Option<u64>) -> Map<String, Value> {
    let keys: Vec<&str> = std::iter::once(leftover.title.as_str()).chain(files.iter().map(String::as_str)).collect();
    let mut request = json!({ "title": leftover.title, "text": leftover.detail, "effect": "new_waves", "keys": keys, "author": "wave" });
    if let Some(origin) = origin {
        request["origin"] = json!(origin);
    }
    request.as_object().cloned().unwrap_or_default()
}

/// Junta o achado `leftover`, apontado pela onda `wave`, à versão `draft` da
/// tarefa da mesma regra: a parte do agente ganha uma linha com o título, o
/// detalhe numa linha só e os lugares dele, no idioma `lang`; os arquivos
/// `files` do achado entram nos da tarefa; a tarefa sem prioridade ganha a do
/// achado ([`leftover_priority`]), salvo quando ele é limpeza; os lugares do
/// achado entram nos da tarefa, que a entrega dela muda; e a marca de
/// limpeza fica só quando o achado também a traz — a tarefa que passa a mudar
/// comportamento deixa de ser limpeza. Os critérios da tarefa ficam como
/// estão.
fn join(draft: &mut Map<String, Value>, wave: u64, leftover: &Leftover, files: &BTreeSet<String>, lang: Locale) {
    let line = said_by(wave, JOINED_BY_RULE, lang)
        .replace("{title}", &leftover.title)
        .replace("{detail}", &text::one_line(&leftover.detail));
    append_agent_line(draft, &format!("{line}{}", places_note(leftover, lang)));
    let mut paths: Vec<Value> = draft.get(FILES).and_then(Value::as_array).cloned().unwrap_or_default();
    for path in files {
        let file = json!({ "path": path });
        if !paths.contains(&file) {
            paths.push(file);
        }
    }
    draft.insert(FILES.into(), json!(paths));
    let mut places: Vec<Value> = draft.get(PLACES).and_then(Value::as_array).cloned().unwrap_or_default();
    for place in &leftover.places {
        let place = json!(place);
        if !places.contains(&place) {
            places.push(place);
        }
    }
    draft.insert(PLACES.into(), json!(places));
    if !leftover.cleanup {
        draft.remove(CLEANUP);
        if draft.get(PRIORITY).and_then(Value::as_str).is_none_or(|said| said.trim().is_empty()) {
            draft.insert(PRIORITY.into(), json!(leftover_priority(wave, lang)));
        }
    }
}

/// A linha da tarefa nova da regra que o achado `leftover` traz: a regra e
/// os lugares dela, no idioma `lang`. É por ela que o agente da onda sabe a
/// regra e onde ela vale.
fn rule_line(leftover: &Leftover, lang: Locale) -> String {
    format!("{}{}", translate("finding.rule", lang).replace("{rule}", &leftover.rule), places_note(leftover, lang))
}

/// Os lugares da regra que o achado `leftover` diz, entre crases, depois de
/// um espaço, no idioma `lang`.
fn places_note(leftover: &Leftover, lang: Locale) -> String {
    let listed = leftover.places.iter().map(|place| format!("`{place}`")).collect::<Vec<_>>().join(", ");
    format!(" {}", translate("finding.places", lang).replace("{places}", &listed))
}

/// A prioridade da tarefa que a sobra da onda `wave` grava, no idioma
/// `lang`: o erro achado na obra vai à frente das tarefas antigas.
fn leftover_priority(wave: u64, lang: Locale) -> String {
    said_by(wave, PRIORITY_SAID, lang)
}

/// O nome do grupo de assunto da tarefa `task`: o que ela grava em
/// [`GROUP`]; sem ele, o código dela, lido em `log`, porque a tarefa de
/// referência dá nome ao grupo que nasce dela.
pub(super) fn group_name(log: &SpecLog, task: &SpecEvent) -> String {
    task.fields
        .get(GROUP)
        .and_then(|group| group.get("name"))
        .and_then(Value::as_str)
        .map_or_else(|| log.codes().get(&task.id).cloned().unwrap_or_else(|| task.id.to_string()), str::to_string)
}

/// O grupo da tarefa nova que fica no assunto da tarefa `near`: o nome do
/// grupo dela e a posição depois da última tarefa ainda viva do grupo — a de
/// referência sem grupo gravado conta como a primeira.
pub(super) fn group_after(log: &SpecLog, near: u64) -> Option<Value> {
    let reference = log.get(near)?;
    let name = group_name(log, reference);
    let position = |task: &SpecEvent| task.fields.get(GROUP).and_then(|group| group.get("position")).and_then(Value::as_u64).unwrap_or(1);
    let last = log
        .visible()
        .into_iter()
        .filter(|event| event.event_type == TASK && (event.id == near || (group_name(log, event) == name && event.fields.contains_key(GROUP))))
        .map(position)
        .max()
        .unwrap_or(1);
    Some(json!({ "name": name, "position": last + 1 }))
}

/// Os arquivos da sobra `leftover`, apontada pela onda `wave`, sem repetir:
/// os que ela declara em `files`, quando traz a lista; sem a lista, os que o
/// detalhe cita entre crases e que existem no repositório `root` ou na cópia
/// da onda, lida em `log` — o exemplo citado no texto não vira arquivo da
/// tarefa quando a lista existe. O caminho de dentro da cópia vira o
/// relativo ao repositório, e o absoluto de fora dela não entra.
fn leftover_files(root: &Path, log: &SpecLog, wave: u64, leftover: &Leftover) -> Vec<String> {
    let copy = wave_prompt::recorded_copy(log, wave).map(|copy| Path::new(&copy.path).to_path_buf());
    let on_disk = |path: &str| root.join(path).exists() || copy.as_ref().is_some_and(|copy| copy.join(path).exists());
    let (cited, check): (Vec<String>, bool) = match &leftover.files {
        Some(declared) => (declared.clone(), false),
        None => (cited_paths(&leftover.detail), true),
    };
    let mut files: Vec<String> = Vec::new();
    for cited in cited {
        let path = own_copy_relative(log, wave, &cited.replace('\\', "/"));
        if !path.is_empty() && !Path::new(&path).is_absolute() && (!check || on_disk(&path)) && !files.contains(&path) {
            files.push(path);
        }
    }
    files
}

/// A tarefa nova que a sobra vira: o título e o detalhe dela, sem
/// dependência, com o autor da onda, e os arquivos dela
/// ([`leftover_files`]). Ela não herda os critérios da onda que a apontou:
/// quem a liga ao pedido da sobra é a rodada, ao gravar os dois. A sobra
/// marcada como limpeza dá a tarefa com a mesma marca, sem prioridade; a sem
/// marca, a tarefa sem o campo e com a prioridade da sobra
/// ([`leftover_priority`]). A sobra sem arquivo nenhum dá a tarefa presa (`blocked_by`),
/// com o motivo no idioma `lang`: um agente partiria sem saber o que tocar,
/// e a revisão não teria escopo a conferir. A fila não a despacha até uma
/// versão nova, gravada por quem conduz a obra com os arquivos e as
/// dependências, sair sem a trava.
fn leftover_task(root: &Path, log: &SpecLog, wave: u64, leftover: &Leftover, lang: Locale) -> Map<String, Value> {
    let files: Vec<Value> =
        leftover_files(root, log, wave, leftover).into_iter().map(|path| json!({ "path": path })).collect();
    let unplanned = files.is_empty();
    let task = json!({
        "title": leftover.title,
        "text": leftover.detail,
        "files": files,
        "depends_on": [],
        "author": "wave",
    });
    let mut task = task.as_object().cloned().unwrap_or_default();
    if unplanned {
        task.insert("blocked_by".into(), json!(translate("round.leftover_without_files", lang)));
    }
    if leftover.cleanup {
        task.insert(CLEANUP.into(), json!(true));
    } else {
        task.insert(PRIORITY.into(), json!(leftover_priority(wave, lang)));
    }
    task
}

#[cfg(test)]
mod tests {
    use mustard_core::domain::spec_events::parse_log;
    use mustard_core::domain::spec_state::PhaseWriter;
    use mustard_core::io::spec_events as store;
    use tempfile::tempdir;

    use super::super::tests::{approved, returned, round, waves_in};
    use super::*;

    /// A spec `x` como está gravada agora.
    fn spec_x(root: &Path) -> SpecLog {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
    }

    /// Os eventos do tipo `kind` que a rodada `out` gravou, lidos na spec.
    fn recorded(root: &Path, out: &Value, kind: &str) -> Vec<SpecEvent> {
        let log = spec_x(root);
        out["recorded"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["type"] == json!(kind))
            .map(|r| log.get(r["id"].as_u64().unwrap()).cloned().unwrap_or_else(|| panic!("{out}")))
            .collect()
    }

    /// A spec `x` gravada à mão com as linhas `events`, já lida.
    fn log_of(events: &[Value]) -> SpecLog {
        parse_log(&events.iter().map(Value::to_string).collect::<Vec<_>>().join("\n"))
    }

    /// Os eventos do tipo `kind` entre os que os achados viraram.
    fn of_kind<'a>(events: &'a [LeftoverEvent], kind: &str) -> Vec<&'a LeftoverEvent> {
        events.iter().filter(|event| event.kind == kind).collect()
    }

    /// O achado que quebra a regra `rule` nos lugares `places`, com o título
    /// `title` e os arquivos `files` a mudar.
    fn ruled(title: &str, rule: &str, files: &[&str], places: &[&str]) -> Leftover {
        Leftover {
            title: title.into(),
            detail: format!("{title}."),
            files: Some(files.iter().map(|file| (*file).to_string()).collect()),
            cleanup: false,
            rule: rule.into(),
            places: places.iter().map(|place| (*place).to_string()).collect(),
        }
    }

    /// O achado da volta, como a onda o grava: o título, o detalhe, a regra
    /// e os lugares.
    fn finding(title: &str, detail: &str, rule: &str, places: &[&str]) -> Value {
        json!({"title": title, "detail": detail, "rule": rule, "places": places})
    }

    /// Uma tarefa do backlog faz valer a regra da leitura pelo mapa e é
    /// limpeza. O achado de limpeza da mesma regra a deixa limpeza; o que muda
    /// comportamento tira a marca e dá à tarefa a prioridade do achado.
    #[test]
    fn a_finding_that_changes_behaviour_takes_the_cleanup_mark_off_the_task_of_its_rule() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let reading = "Toda leitura passa pelo mapa";
        let log = log_of(&[json!({"v":1,"id":1,"type":"task","title":"Ler pelo mapa","text":"Ler.","files":[{"path":"src/c.rs"}],
            "depends_on":[],"rule":reading,"cleanup":true})]);
        let tidy = Leftover { cleanup: true, ..ruled("Comentário velho", reading, &["src/c.rs"], &["src/c.rs:1"]) };
        let events = leftover_events(root, &log, &[(1, &tidy)], &BTreeMap::new(), Locale::PtBr);
        let task = of_kind(&events, TASK)[0];
        assert_eq!(task.draft.get(CLEANUP), Some(&json!(true)), "limpeza sobre limpeza segue limpeza: {:?}", task.draft);

        let behaviour = ruled("A leitura abre o arquivo", reading, &["src/c.rs"], &["src/c.rs:1"]);
        let events = leftover_events(root, &log, &[(1, &behaviour)], &BTreeMap::new(), Locale::PtBr);
        let task = of_kind(&events, TASK)[0];
        assert!(!task.draft.contains_key(CLEANUP), "o achado muda comportamento: {:?}", task.draft);
        assert_eq!(task.draft.get(PRIORITY), Some(&json!(leftover_priority(1, Locale::PtBr))), "{:?}", task.draft);
    }

    /// A onda que relata os achados tem critério. A tarefa nova que um achado
    /// vira não cobre esse critério, e a tarefa aberta da mesma regra que
    /// recebe outro achado segue cobrindo só o que já cobria: cada uma passa
    /// a cobrir só o pedido do próprio achado.
    #[test]
    fn a_leftover_task_never_inherits_the_wave_criteria() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let log = log_of(&[
            json!({"v":1,"id":1,"type":"criterion","when":"a","then":"b","proof":"npm test -- a","form":"ubiquitous"}),
            json!({"v":1,"id":2,"type":"criterion","when":"c","then":"d","proof":"npm test -- b","form":"ubiquitous"}),
            json!({"v":1,"id":3,"type":"wave","n":1,"text":"t","criteria":[1],"done_when":"d"}),
            json!({"v":1,"id":4,"type":"task","title":"B","text":"B.","files":[{"path":"src/b.ts"}],"depends_on":[],"covers":[2],"rule":"O total arredonda"}),
        ]);
        let born = ruled("A some", "A soma guarda o sinal", &["src/a.ts"], &["src/a.ts:1"]);
        let joining = ruled("B some", "O total arredonda", &["src/b.ts"], &["src/b.ts:1"]);

        let events = leftover_events(root, &log, &[(1, &born), (1, &joining)], &BTreeMap::new(), Locale::PtBr);
        let tasks = of_kind(&events, TASK);
        assert_eq!(tasks.len(), 2);
        let new = tasks.iter().find(|task| !task.draft.contains_key("replaces")).unwrap();
        assert!(!new.draft.contains_key("covers"), "a tarefa nova não herda o critério da onda: {:?}", new.draft);
        assert_eq!(new.asks, [0]);
        let open = tasks.iter().find(|task| task.draft.contains_key("replaces")).unwrap();
        assert_eq!(open.draft.get("covers"), Some(&json!([2])), "a tarefa aberta não ganha o critério da onda: {:?}", open.draft);
        assert_eq!(open.asks, [1]);
    }

    /// O achado que declara os arquivos a mudar dá a tarefa só com eles: os
    /// caminhos que o texto cita como exemplo, mesmo existindo, ficam de
    /// fora. Sem a lista, o texto ainda dá os arquivos.
    #[test]
    fn a_leftover_with_declared_files_ignores_the_paths_its_text_cites() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        for file in ["src/soma.ts", "web/layout.css", "scripts/install.ps1", "apps/rt/Cargo.toml"] {
            std::fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            std::fs::write(root.join(file), "x\n").unwrap();
        }
        let log = log_of(&[json!({"v":1,"id":1,"type":"wave","n":1,"text":"t","criteria":[],"done_when":"d"})]);
        let detail = "O acerto errado aparece em `web/layout.css`, `scripts/install.ps1` e `apps/rt/Cargo.toml`; o conserto é em `src/soma.ts`.";
        let declared = Leftover { detail: detail.into(), ..ruled("Acerto errado", "O acerto vale em todo lugar", &["src/soma.ts"], &["src/soma.ts:1"]) };

        let events = leftover_events(root, &log, &[(1, &declared)], &BTreeMap::new(), Locale::PtBr);
        let task = of_kind(&events, TASK)[0];
        assert_eq!(task.draft.get("files"), Some(&json!([{"path": "src/soma.ts"}])), "{:?}", task.draft);
        assert!(!task.draft.contains_key("blocked_by"), "{:?}", task.draft);

        let cited = Leftover { files: None, ..declared };
        let events = leftover_events(root, &log, &[(1, &cited)], &BTreeMap::new(), Locale::PtBr);
        let files = of_kind(&events, TASK)[0].draft.get("files").and_then(Value::as_array).map_or(0, Vec::len);
        assert_eq!(files, 4, "sem a lista, o texto ainda dá os arquivos");
    }

    /// A onda 1, com critério, volta com um achado que declara um arquivo
    /// TypeScript. A rodada grava um pedido com o título e o relato dele,
    /// para ondas novas, que aponta a entrega da onda como origem, e uma
    /// tarefa que cobre só esse pedido, pelo número gravado, com a regra.
    #[test]
    fn a_leftover_becomes_a_request_and_a_task_covering_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/soma.ts"), "export const soma = 1;\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(root.join("src/a.rs"), "fn one() {}\n// A soma saiu.\n").unwrap();
        let detail = "A soma perde o sinal. Prova: `npm test -- soma`.";
        let mut found = finding("A soma perde o sinal", detail, "A soma guarda o sinal", &["src/soma.ts:1"]);
        found["files"] = json!(["src/soma.ts"]);
        let wrote = returned(root, json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a soma sai", "leftovers": [found]}));
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let requests = recorded(root, &out, REQUEST);
        assert_eq!(requests.len(), 1, "{out}");
        let request = &requests[0];
        assert_eq!(request.str_field("title"), Some("A soma perde o sinal"), "{:?}", request.fields);
        assert_eq!(request.str_field("text"), Some(detail), "{:?}", request.fields);
        assert_eq!(request.str_field("effect"), Some("new_waves"), "{:?}", request.fields);
        let delivered = recorded(root, &out, "delivered");
        assert_eq!(request.int("origin"), Some(delivered[0].id), "o pedido nasce da entrega: {:?}", request.fields);
        let tasks = recorded(root, &out, TASK);
        assert_eq!(tasks.len(), 1, "{out}");
        assert_eq!(tasks[0].fields["covers"], json!([request.id]), "a tarefa cobre só o pedido do achado: {:?}", tasks[0].fields);
        assert_eq!(tasks[0].fields["files"], json!([{"path": "src/soma.ts"}]), "{:?}", tasks[0].fields);
        assert_eq!(tasks[0].str_field(RULE), Some("A soma guarda o sinal"), "{:?}", tasks[0].fields);
    }

    /// As ondas 1 e 2 voltam juntas, antes da mesma rodada, com um achado
    /// sem arquivo cada uma, da mesma regra. A rodada grava um pedido por
    /// achado e uma tarefa só, presa a planejar, e o achado da onda 2 entra
    /// nela como linha.
    #[test]
    fn findings_of_one_rule_from_two_returns_in_one_round_make_one_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        let rule = "As cópias cabem no disco";
        for (wave, file) in [(1, "src/a.rs"), (2, "src/b.rs")] {
            std::fs::write(root.join(file), "fn one() {}\n// Saiu.\n").unwrap();
            let found = finding("O disco enche", "As cópias lotam o disco durante a obra.", rule, &[file]);
            let wrote = returned(root, json!({"wave": wave, "text": "Saiu.", "files": [file], "commit": "sai", "leftovers": [found]}));
            assert_eq!(wrote["ok"], json!(true), "{wrote}");
        }
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(recorded(root, &out, REQUEST).len(), 2, "{out}");
        let tasks = recorded(root, &out, TASK);
        assert_eq!(tasks.len(), 1, "{out}");
        assert!(tasks[0].str_field("blocked_by").is_some(), "{:?}", tasks[0].fields);
        let joined = translate("finding.joined_by_rule", Locale::PtBr)
            .replace("{wave}", "2")
            .replace("{title}", "O disco enche")
            .replace("{detail}", "As cópias lotam o disco durante a obra.");
        assert!(tasks[0].str_field("agent").is_some_and(|agent| agent.contains(&joined)), "{:?}", tasks[0].fields);
    }

    /// A volta traz um achado que não cita arquivo nenhum. A tarefa que ele
    /// vira fica presa, com o motivo, a fila a mostra a planejar, e a rodada
    /// seguinte não despacha onda nenhuma com ela. Só depois de quem conduz a
    /// obra gravar uma versão nova com os arquivos, sem a trava, ela sai numa
    /// onda.
    #[test]
    fn a_leftover_without_files_waits_for_planning() {
        // A onda que sai do backlog nasce do mapa que o scan grava: sem o
        // scan compilado junto, nenhuma sairia, e o teste passaria à toa.
        assert!(
            mustard_core::Scan::locate().is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/c.rs"), "fn main() {}\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(root.join("src/a.rs"), "fn one() {}\n// A soma saiu.\n").unwrap();
        let found = finding("O disco enche", "As cópias lotam o disco durante a obra.", "As cópias cabem no disco", &["src/a.rs"]);
        let wrote = returned(root, json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a soma sai", "leftovers": [found]}));
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let out = round(root, "x", None);
        let tasks = recorded(root, &out, "task");
        assert_eq!(tasks.len(), 1, "{out}");
        let task = &tasks[0];
        assert_eq!(task.fields["files"], json!([]), "{:?}", task.fields);
        assert_eq!(task.str_field("blocked_by"), Some(translate("round.leftover_without_files", Locale::PtBr)), "{:?}", task.fields);
        let code = spec_x(root).codes()[&task.id].clone();
        let queue = super::super::scheduling::snapshot(root, "x", &spec_x(root));
        let row = queue["tasks"].as_array().unwrap().iter().find(|row| row["task"] == json!(code)).cloned();
        assert_eq!(row.map(|row| row["reason"].clone()), Some(json!("needs-planning")), "{queue}");

        let held = round(root, "x", None);
        assert!(waves_in(&held, "dispatch").is_empty(), "a tarefa sem arquivo não sai como onda: {held}");

        let mut planned = backlog_return(task);
        planned.remove("blocked_by");
        planned.insert("files".into(), json!([{"path": "src/c.rs"}]));
        crate::commands::spec_events::write::record(root, "x", "task", planned, PhaseWriter::Binary).unwrap();
        let next = round(root, "x", None);
        assert_eq!(waves_in(&next, "dispatch"), vec![2], "planejada, a tarefa sai: {next}");
    }

    /// A volta da onda 1 traz três achados: dois da regra da leitura pelo
    /// mapa, em `src/a.rs` e em `src/b.rs`, e um da regra da citação, também
    /// em `src/a.rs`, onde já espera uma tarefa aberta sem regra. Os três
    /// viram três pedidos e duas tarefas novas — a da leitura cobre os dois
    /// primeiros pedidos, com os dois arquivos, a regra e uma linha do
    /// segundo achado; a da citação cobre o terceiro. A tarefa aberta de
    /// `src/a.rs` não ganha versão nova.
    #[test]
    fn findings_of_one_rule_make_one_task_and_of_two_rules_two_tasks() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let log = log_of(&[json!({"v":1,"id":1,"type":"task","title":"O índice some","text":"Some.",
            "files":[{"path":"src/a.rs"}],"depends_on":[]})]);
        let reading = "Toda leitura passa pelo mapa";
        let findings = [
            ruled("A leitura de a.rs abre o arquivo", reading, &["src/a.rs"], &["src/a.rs:3"]),
            ruled("A leitura de b.rs abre o arquivo", "toda leitura passa pelo mapa.", &["src/b.rs"], &["src/b.rs:7"]),
            ruled("A citação de a.rs não confere a pasta", "A gravação confere a citação", &["src/a.rs"], &["src/a.rs:9"]),
        ];
        let found: Vec<(u64, &Leftover)> = findings.iter().map(|leftover| (1, leftover)).collect();

        let events = leftover_events(root, &log, &found, &BTreeMap::new(), Locale::PtBr);

        assert_eq!(of_kind(&events, REQUEST).len(), 3);
        let tasks = of_kind(&events, TASK);
        assert_eq!(tasks.len(), 2, "duas regras, duas tarefas");
        assert!(tasks.iter().all(|task| !task.draft.contains_key("replaces")), "a tarefa aberta do mesmo arquivo não recebe achado");
        let (one, other) = (tasks[0], tasks[1]);
        assert_eq!(one.asks, [0, 1]);
        assert_eq!(one.draft.get(RULE), Some(&json!(reading)));
        assert_eq!(one.draft.get(FILES), Some(&json!([{"path": "src/a.rs"}, {"path": "src/b.rs"}])));
        let agent = one.draft.get("agent").and_then(Value::as_str).unwrap_or_default();
        let rule = translate("finding.rule", Locale::PtBr).replace("{rule}", reading);
        let second = translate("finding.joined_by_rule", Locale::PtBr)
            .replace("{wave}", "1")
            .replace("{title}", "A leitura de b.rs abre o arquivo")
            .replace("{detail}", "A leitura de b.rs abre o arquivo.");
        assert!(agent.contains(&rule) && agent.contains("`src/a.rs:3`"), "{agent}");
        assert!(agent.contains(&second) && agent.contains("`src/b.rs:7`"), "{agent}");
        assert_eq!(other.asks, [2]);
        assert_eq!(other.draft.get(RULE), Some(&json!("A gravação confere a citação")));
        assert_eq!(other.draft.get(FILES), Some(&json!([{"path": "src/a.rs"}])));
    }

    /// Uma tarefa do backlog faz valer a regra da leitura pelo mapa, e outra
    /// tarefa, já numa onda, faz valer a da citação. A volta traz um achado de
    /// cada regra: o da leitura entra na tarefa do backlog, numa versão nova
    /// com a linha dele, o arquivo dele e o pedido com a marca da sobra, sem
    /// tarefa nova; o da citação vira tarefa nova, porque a da onda não espera
    /// no backlog.
    #[test]
    fn a_finding_joins_the_backlog_task_of_its_rule_instead_of_making_a_new_one() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let reading = "Toda leitura passa pelo mapa";
        let log = log_of(&[
            json!({"v":1,"id":1,"type":"task","title":"Ler pelo mapa","text":"Ler.","files":[{"path":"src/c.rs"}],"depends_on":[],"rule":reading}),
            json!({"v":1,"id":2,"type":"wave","n":1,"text":"t","criteria":[],"done_when":"d"}),
            json!({"v":1,"id":3,"type":"task","wave":1,"title":"Conferir","text":"Conferir.","files":[{"path":"src/d.rs"}],"depends_on":[],
                "rule":"A gravação confere a citação"}),
        ]);
        let again = ruled("A leitura de a.rs abre o arquivo", "TODA leitura passa pelo mapa", &["src/a.rs"], &["src/a.rs:3"]);
        let cited = ruled("A citação de d.rs não confere", "A gravação confere a citação", &["src/d.rs"], &["src/d.rs:1"]);

        let events = leftover_events(root, &log, &[(2, &again), (2, &cited)], &BTreeMap::new(), Locale::PtBr);

        let tasks = of_kind(&events, TASK);
        assert_eq!(tasks.len(), 2);
        let version = tasks.iter().find(|task| task.draft.get("replaces") == Some(&json!(1))).expect("a versão nova da tarefa da regra");
        assert_eq!(version.asks, [0]);
        assert_eq!(version.draft.get(FILES), Some(&json!([{"path": "src/c.rs"}, {"path": "src/a.rs"}])));
        assert!(version.draft.get("agent").and_then(Value::as_str).is_some_and(|agent| agent.contains("A leitura de a.rs abre o arquivo")));
        assert_eq!(of_kind(&events, REQUEST)[0].draft.get("title"), Some(&json!(marked_title(2, "A leitura de a.rs abre o arquivo", Locale::PtBr))));
        let born = tasks.iter().find(|task| !task.draft.contains_key("replaces")).expect("a tarefa nova da citação");
        assert_eq!(born.asks, [1]);
    }

    /// A regra que não é texto, e os lugares que não são uma lista de textos,
    /// recusam a leitura da volta pelo número do achado, como os lugares só
    /// em branco, que valem nenhum; a regra e cada lugar chegam sem os
    /// espaços das pontas.
    #[test]
    fn a_finding_rule_or_places_of_the_wrong_type_is_refused() {
        let base = finding("A leitura abre o arquivo", "Abre.", "Toda leitura passa pelo mapa", &["src/a.rs:3"]);
        for (field, wrong, expected) in [("rule", json!(3), Kind::Text), ("places", json!("a.rs:1"), Kind::Texts), ("places", json!([1]), Kind::Texts)] {
            let mut item = base.clone();
            item[field] = wrong;
            let refused = leftovers_of("delivered", &[base.clone(), item]).err();
            assert_eq!(refused, Some(Refusal::InvalidValue { event_type: "delivered".into(), field: format!("leftovers[2].{field}"), expected }));
        }
        let mut blank = base.clone();
        blank["places"] = json!(["  "]);
        let refused = leftovers_of("verdict", &[base.clone(), blank]).err();
        assert_eq!(refused, Some(Refusal::LeftoverFieldMissing { finding: 2, field: PLACES.into() }));
        let mut item = base;
        item["rule"] = json!("  Toda leitura passa pelo mapa ");
        item["places"] = json!([" src/a.rs:3 "]);
        let read = leftovers_of("verdict", &[item]).unwrap();
        assert_eq!((read[0].rule.as_str(), read[0].places.as_slice()), ("Toda leitura passa pelo mapa", &["src/a.rs:3".to_string()][..]));
    }

    /// O achado que entra na tarefa aberta da mesma regra dá o pedido com a
    /// marca da sobra no título. Quando a versão nova da tarefa sai numa
    /// onda, cobrindo esse pedido, o pedido da onda mostra a marca no que ela
    /// atende; o achado que vira tarefa nova dá o pedido com o título dele,
    /// sem marca.
    #[test]
    fn a_task_that_gains_a_leftover_shows_the_leftover_mark_in_the_wave_request() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let task = json!({"v":1,"id":1,"type":"task","title":"O índice some","text":"Some.","files":[{"path":"src/commit.ts"}],"depends_on":[],
            "rule":"A leitura tem índice"});
        let log = log_of(std::slice::from_ref(&task));
        let joining = ruled("A leitura para sem índice", "A leitura tem índice", &["src/commit.ts"], &["src/commit.ts:1"]);
        let fresh = ruled("O log não gira", "O log gira por dia", &["src/log.ts"], &["src/log.ts:1"]);
        let events = leftover_events(root, &log, &[(1, &joining), (1, &fresh)], &BTreeMap::new(), Locale::PtBr);
        let requests = of_kind(&events, REQUEST);
        let marked = marked_title(1, "A leitura para sem índice", Locale::PtBr);
        assert_eq!(requests[0].draft.get("title"), Some(&json!(marked)));
        assert_eq!(requests[1].draft.get("title"), Some(&json!("O log não gira")), "a tarefa nova já leva o título do achado");
        let version = of_kind(&events, TASK).into_iter().find(|task| task.draft.get("replaces") == Some(&json!(1))).unwrap();

        let event = |id: u64, kind: &str, draft: &Map<String, Value>| {
            let mut event = draft.clone();
            event.extend([("v".into(), json!(1)), ("id".into(), json!(id)), ("type".into(), json!(kind))]);
            Value::Object(event)
        };
        let mut sent = version.draft.clone();
        sent.extend([("wave".into(), json!(2)), ("covers".into(), json!([3]))]);
        let planned = log_of(&[
            task,
            json!({"v":1,"id":2,"type":"wave","n":2,"text":"A onda","criteria":[],"done_when":"passa"}),
            event(3, REQUEST, &requests[0].draft),
            event(4, TASK, &sent),
        ]);
        let code = planned.codes()[&3].clone();
        let built = wave_prompt::prompts(root, "x", &planned, Locale::PtBr, &wave_prompt::Flight::default(), &planned.planned_waves());
        let attends = translate("prompt.step.attends", Locale::PtBr).replace("{item}", "");
        let text = &built.iter().find(|prompt| prompt.wave == 2).unwrap().text;
        assert!(
            text.lines().any(|line| line.contains(attends.trim()) && line.contains(&code) && line.contains(&marked)),
            "a tarefa mostra o achado que ganhou: {text}"
        );
    }
}
