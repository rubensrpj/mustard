//! A resposta de uma volta pelo combinado: cada item que a entrega de uma
//! onda ou o veredito final responde em `agreed`, pelo número vigente, e o
//! item não cumprido que vira trabalho no backlog — uma vez só: a tarefa
//! ainda por entregar que já cobre o item basta, e nenhuma outra nasce, e o
//! item que a análise da onda tirou do pedido dela não vira tarefa nenhuma.
//! O item não cumprido de uma volta e o da revisão final passam pelo
//! classificador das atividades ([`task_triage`]), como a sobra, num pedido
//! só por rodada: o que uma tarefa aberta já cobre vira nota que a aponta; o
//! de mesmo sentido de uma tarefa aberta entra nela, e a tarefa pede a
//! reescrita ao modelo pequeno da sessão; o novo vira tarefa, no grupo da
//! tarefa de assunto mais próximo. A tarefa devolvida que nasceu do item o
//! recebe sem pergunta. Sem leitura confiante, vale a regra nativa: na volta
//! que deixou tarefas por fazer, o item entra numa delas, na versão que volta
//! ao backlog; sem tarefa devolvida, nasce como tarefa nova.
//!
//! [`task_triage`]: crate::shared::task_triage

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::spec_events::{Refusal, SpecEvent, SpecLog};
use mustard_core::domain::wave_prompt as agreed_prompt;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::leftovers::{group_after, MeaningJoin, GROUP};
use super::queue::{backlog_left, task_files, tasks_not_delivered};
use super::report::{agreed_item_id, backlog_return, dispatched_at, WaveReport};
use super::stops::append_agent_line;
use crate::shared::task_triage::{self, Candidate, Destination, Item, Summary};

/// O que [`settle_agreed`] devolve: cada item não cumprido que ainda pede
/// trabalho e o código de cada item esperado que ficou sem resposta.
pub(super) type SettledAgreed = (Vec<Unmet>, Vec<String>);

/// Um evento que o item não cumprido de uma volta vira fora da tarefa que ela
/// devolveu ([`join_unmet`]): a onda que o respondeu, o tipo do evento — a
/// nota ou a tarefa — e o rascunho.
pub(super) type UnmetEvent = (u64, &'static str, Map<String, Value>);

/// Cada tarefa que ganhou pelo sentido itens não cumpridos, com o número de
/// cada item: é ela que pede a reescrita ([`unmet_meaning_joins`]).
pub(super) type UnmetJoins = Vec<(JoinedAt, Vec<u64>)>;

/// Quem respondeu pelo item não cumprido: a volta de uma onda, pelo número,
/// ou a revisão final.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum AnsweredBy {
    Wave(u64),
    Review,
}

impl AnsweredBy {
    /// O autor do que o item vira.
    fn author(self) -> &'static str {
        match self {
            Self::Wave(_) => "wave",
            Self::Review => "review",
        }
    }

    /// O texto `wave_key`, com a onda, ou o `review_key`, da revisão final,
    /// no idioma `lang`.
    fn said(self, wave_key: &str, review_key: &str, lang: Locale) -> String {
        match self {
            Self::Wave(wave) => translate(wave_key, lang).replace("{wave}", &wave.to_string()),
            Self::Review => translate(review_key, lang).to_string(),
        }
    }
}

/// Onde está a tarefa que ganhou pelo sentido um item não cumprido: a versão
/// devolvida, pela posição entre as que voltam ao backlog, ou o evento, pela
/// posição entre os que os itens viram.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum JoinedAt {
    Returned(usize),
    Event(usize),
}

/// Os itens combinados que o pedido da onda `wave` levou: a lista que o envio
/// que a despachou ([`dispatched_at`]) gravou do que o pedido lia, pela mesma
/// conta que montou "O que obedecer" ([`agreed_prompt::sent_agreed`]) — os
/// tipos do bloco do combinado que o veredito final também responde: regra,
/// limite, contrato, erro, caso de borda, fora do escopo e decisão. A entrega
/// da onda responde por cada um deles. O item que a escolha da onda tirou do
/// pedido não está na lista e não é cobrado, também depois de um reenvio. O
/// envio gravado antes da lista fica com a conta da montagem
/// ([`agreed_prompt::dispatch_items`]), com a escolha que ele gravou.
///
/// A lista é a do envio, não a de agora: a leitura é feita sobre a spec como
/// estava nele, só com os eventos de número até o dele — os números da spec
/// só crescem. O item combinado gravado entre o envio e a volta não é
/// cobrado, porque o pedido não o levou. O que o pedido levou e ganhou versão
/// nova depois é cobrado pela versão de agora, pelo mesmo código; o que saiu
/// da spec depois, não.
pub(super) fn request_agreed(log: &SpecLog, wave: u64) -> Vec<&SpecEvent> {
    let then = as_dispatched(log, wave);
    let then_codes = then.codes();
    let sent = dispatched_at(log, wave).and_then(|id| then.get(id)).and_then(|send| agreed_prompt::sent_agreed(&then, send));
    let carried = sent.unwrap_or_else(|| agreed_prompt::carried_agreed(agreed_prompt::dispatch_items(&then, wave, None)));
    let carried: Vec<&String> = carried.iter().filter_map(|item| then_codes.get(&item.id)).collect();
    let codes = log.codes();
    let agreed = agreed_prompt::all_agreed(log);
    carried
        .into_iter()
        .filter_map(|code| agreed.iter().copied().find(|item| codes.get(&item.id) == Some(code)))
        .collect()
}

/// A spec como estava no envio que despachou a onda `wave`: só os eventos de
/// número até o dele. Sem envio, a spec inteira.
pub(super) fn as_dispatched(log: &SpecLog, wave: u64) -> SpecLog {
    let sent = dispatched_at(log, wave).unwrap_or(u64::MAX);
    SpecLog { events: log.events.iter().filter(|e| e.id <= sent).cloned().collect(), ..SpecLog::default() }
}

/// Os códigos dos itens que alguma tarefa ainda por entregar cobre
/// ([`tasks_not_delivered`]), fora as das ondas em `returning`. Pelo código,
/// a tarefa que cobre uma versão antiga do item cobre também a de agora.
pub(super) fn covered_codes(log: &SpecLog, returning: &BTreeSet<u64>) -> BTreeSet<String> {
    let codes = log.codes();
    tasks_not_delivered(log, returning)
        .into_iter()
        .filter_map(|id| log.get(id))
        .flat_map(|task| task.ints("covers"))
        .filter_map(|id| codes.get(&id).cloned())
        .collect()
}

/// Os códigos dos itens que a análise da onda `wave` tirou do pedido dela, o
/// do projeto todo e o dos arquivos da onda: a escolha gravada no envio que
/// a despachou, lida sobre a spec como estava nele ([`as_dispatched`]) e
/// reduzida aos candidatos de então, a mesma que monta o pedido
/// ([`agreed_prompt::dispatch_items`]). O pedido não levou esses itens: a
/// entrega não é cobrada por eles, e a resposta que der a algum deles não
/// cria tarefa.
fn removed_by_analysis(log: &SpecLog, wave: u64) -> BTreeSet<String> {
    let then = as_dispatched(log, wave);
    let Some(choice) = agreed_prompt::recorded_choice(&then, wave) else { return BTreeSet::new() };
    let codes = then.codes();
    let choice = choice.within(&agreed_prompt::candidates(&then, wave));
    choice.removed.iter().filter_map(|(id, _)| codes.get(id).cloned()).collect()
}

/// Um item combinado que a resposta não cumpriu e que ainda pede trabalho: o
/// número vigente dele, o que a resposta diz que falta — sem isso, o texto do
/// próprio item — e os arquivos da tarefa que vai refazê-lo, cada um como
/// `{"path": …}`: os que o próprio item declara, como a tarefa não cumprida
/// declara os dela, e depois os que a resposta cita a mais.
pub(super) struct Unmet {
    id: u64,
    text: String,
    files: Vec<Value>,
}

/// Resolve a resposta `agreed` de `draft`: cada item citado vira o número
/// vigente dele. Devolve o número de cada item respondido e, na ordem da
/// resposta, cada item que não vem `met:true` e ainda pede trabalho: fica de
/// fora o de código em `covered`, que alguma tarefa ainda por entregar já
/// cobre, e o de código em `removed`, que a análise da onda tirou do pedido.
/// Cada item devolvido entra em `covered`. O item citado que a spec não tem é
/// recusado.
fn answered_items(
    log: &SpecLog,
    draft: &mut Map<String, Value>,
    covered: &mut BTreeSet<String>,
    removed: &BTreeSet<String>,
) -> Result<(Vec<u64>, Vec<Unmet>), Refusal> {
    let (mut answered, mut unmet) = (Vec::new(), Vec::new());
    let codes = log.codes();
    for item in draft.get_mut("agreed").and_then(Value::as_array_mut).into_iter().flatten() {
        let id = agreed_item_id(log, item.get("item").unwrap_or(&Value::Null))?;
        item["item"] = json!(id);
        answered.push(id);
        if item.get("met").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let code = codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
        if removed.contains(&code) || !covered.insert(code) {
            continue;
        }
        let said = item.get("text").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty());
        let text = said.or_else(|| log.get(id).and_then(|e| e.str_field("text"))).unwrap_or_default().to_string();
        let cited = item.get("files").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str);
        let mut paths: Vec<&str> = Vec::new();
        for path in log.get(id).map(declared_paths).into_iter().flatten().chain(cited) {
            if !path.is_empty() && !paths.contains(&path) {
                paths.push(path);
            }
        }
        let files = paths.into_iter().map(|f| json!({ "path": f })).collect();
        unmet.push(Unmet { id, text, files });
    }
    Ok((answered, unmet))
}

/// Os caminhos que o item `event` declara em `files`, na ordem gravada: o
/// `path` de cada um, como a tarefa grava, ou o texto dele, como a entrega
/// grava.
fn declared_paths(event: &SpecEvent) -> impl Iterator<Item = &str> {
    let files = event.fields.get("files").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    files.iter().filter_map(|file| file.as_str().or_else(|| file.get("path").and_then(Value::as_str)))
}

/// A resposta `agreed` de uma volta — a entrega de uma onda ou o veredito
/// final — resolvida contra os itens combinados que ela precisa responder
/// (`expected`) por [`answered_items`]: devolve cada item que não vem
/// `met:true` e ainda pede trabalho, com o que a resposta diz que falta e os
/// arquivos do item e os que ela cita. O item já em `covered` ou em `removed`
/// fica na resposta como veio e não volta; cada item devolvido entra em
/// `covered`. O destino do item não cumprido sai do classificador — o da volta
/// de uma onda, de [`join_unmet`]; o da revisão final, de
/// [`place_review_unmet`] —, e aqui só se confere o que ficou sem resposta.
/// Devolve também o código de cada item esperado que ficou sem resposta.
pub(super) fn settle_agreed(
    log: &SpecLog,
    draft: &mut Map<String, Value>,
    expected: &[&SpecEvent],
    covered: &mut BTreeSet<String>,
    removed: &BTreeSet<String>,
) -> Result<SettledAgreed, Refusal> {
    let (answered, unmet) = answered_items(log, draft, covered, removed)?;
    let codes = log.codes();
    let missing = expected
        .iter()
        .filter(|item| !answered.contains(&item.id))
        .map(|item| codes.get(&item.id).cloned().unwrap_or_else(|| item.id.to_string()))
        .collect();
    Ok((unmet, missing))
}

/// A tarefa nova que refaz o item não cumprido `item`, de autor `author`: o
/// que falta, os arquivos e o item em `covers`, sem dependência.
fn unmet_task(item: Unmet, author: &str) -> Map<String, Value> {
    let Unmet { id, text, files } = item;
    let task = json!({ "text": text, "files": files, "depends_on": [], "covers": [id], "author": author });
    task.as_object().cloned().unwrap_or_default()
}

/// A resposta `agreed` da volta, já na entrega oficial `draft` dela, com cada
/// item citado pelo número vigente ([`answered_items`]); o item que a spec não
/// tem é recusado. O destino do item não cumprido já saiu de [`join_unmet`].
pub(super) fn settle_wave_agreed(log: &SpecLog, draft: &mut Map<String, Value>) -> Result<(), Refusal> {
    answered_items(log, draft, &mut BTreeSet::new(), &BTreeSet::new()).map(drop)
}

/// Os números que a versão `task` de uma tarefa grava em `key`: a lista ou um
/// número só.
fn listed_ids(task: &Map<String, Value>, key: &str) -> Vec<u64> {
    let many = task.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64);
    many.chain(task.get(key).and_then(Value::as_u64)).collect()
}

/// Põe o item não cumprido `item`, de código `code`, por que `by` respondeu,
/// na versão `task` de uma tarefa: ela passa a cobrir o item, ganha os
/// arquivos do item e os que a resposta cita e, na parte do agente, uma linha
/// com quem respondeu e o que falta, no idioma `lang`.
fn take_unmet(task: &mut Map<String, Value>, by: AnsweredBy, code: &str, item: &Unmet, lang: Locale) {
    let mut covers = listed_ids(task, "covers");
    covers.push(item.id);
    task.insert("covers".into(), json!(covers));
    let mut paths: Vec<Value> = task.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
    for file in &item.files {
        if !paths.contains(file) {
            paths.push(file.clone());
        }
    }
    task.insert("files".into(), json!(paths));
    let said = item.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let line = by.said("round.unmet_joined", "round.unmet_review_joined", lang).replace("{code}", code).replace("{text}", &said);
    append_agent_line(task, &line);
}

/// A nota do item não cumprido `item`, de código `code`, por que `by`
/// respondeu e que o Jev julgou coberto pela tarefa `task`, com a chance
/// `chance`, no idioma `lang`: o título do item, o texto com quem respondeu,
/// o código da tarefa e o que falta, e o motivo na parte do agente. O código
/// da tarefa vai no rótulo e na chave de busca: é por ele que a nota a
/// aponta, sem mudar nada nela.
fn covered_note(log: &SpecLog, (by, code, item): (AnsweredBy, &str, &Unmet), task: &SpecEvent, chance: f64, lang: Locale) -> Map<String, Value> {
    let task_code = log.codes().get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
    let said = item.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = by
        .said("round.unmet_covered", "round.unmet_review_covered", lang)
        .replace("{code}", code)
        .replace("{task}", &task_code)
        .replace("{text}", &said);
    let title = log.get(item.id).and_then(|event| event.str_field("title")).unwrap_or(code);
    let reason = translate("round.unmet_judged", lang).replace("{chance}", &format!("{chance:.2}"));
    let note = json!({ "title": title, "text": text, "keys": [task_code, code], "label": task_code, "agent": reason, "author": by.author() });
    note.as_object().cloned().unwrap_or_default()
}

/// O destino de cada item combinado que as voltas `waves` não cumpriram e que
/// ainda pede trabalho, com o classificador das atividades da spec `spec` de
/// `root` ([`task_triage::jev_ask`]). `returned` traz a versão que volta ao
/// backlog de cada tarefa não feita, com a onda que a devolveu, na ordem de
/// `undone`: o item que entra numa delas entra nessa versão
/// ([`take_unmet`]). Devolve os outros eventos que os itens viram, na ordem
/// dos itens, e cada tarefa que ganhou itens pelo sentido.
pub(super) fn join_unmet(
    root: &Path,
    spec: &str,
    log: &SpecLog,
    waves: &[WaveReport],
    returned: &mut [(u64, Map<String, Value>)],
    lang: Locale,
) -> Result<(Vec<UnmetEvent>, UnmetJoins), Refusal> {
    join_unmet_with(log, waves, returned, lang, &task_triage::jev_ask(root, spec))
}

/// [`join_unmet`] com a pergunta ao Jev feita por `ask`, que devolve a
/// resposta ou `None` quando não há Jev ou ele falhou.
///
/// Fica de fora, como em [`settle_agreed`], o item que uma tarefa ainda por
/// entregar ou uma tarefa devolvida já cobre e o que a análise da onda tirou
/// do pedido; as tarefas das ondas que voltam e das que o conserto delas
/// fecha não contam, porque a entrega as fecha agora. O item que uma volta
/// já respondeu não entra de novo pela seguinte. A volta cujas tarefas por
/// fazer outra onda já levou, todas, não dá destino a item nenhum: ele está
/// no pedido da onda que levou a tarefa, que responde por ele.
///
/// O item de que nasceu uma tarefa devolvida pela mesma volta (`origin`), a
/// ligação que o pedido da onda mostra, entra nela sem pergunta. Os outros
/// vão ao classificador ([`place_unmet`]), com o resumo de cada volta que
/// deixou tarefa por fazer.
fn join_unmet_with(
    log: &SpecLog,
    waves: &[WaveReport],
    returned: &mut [(u64, Map<String, Value>)],
    lang: Locale,
    ask: &dyn Fn(&str) -> Option<Value>,
) -> Result<(Vec<UnmetEvent>, UnmetJoins), Refusal> {
    let codes = log.codes();
    let code_of = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
    let returning: BTreeSet<u64> = waves.iter().flat_map(|wave| std::iter::once(wave.wave).chain(wave.fixes.iter().copied())).collect();
    let mut covered = covered_codes(log, &returning);
    covered.extend(returned.iter().flat_map(|(_, task)| listed_ids(task, "covers")).map(code_of));
    let mut found: Vec<(u64, Unmet)> = Vec::new();
    for wave in waves.iter().filter(|wave| !wave.agreed.is_empty() && !(wave.taken_elsewhere && wave.undone.is_empty())) {
        let mut answer = Map::from_iter([("agreed".to_string(), json!(wave.agreed))]);
        let (_, unmet) = answered_items(log, &mut answer, &mut covered, &removed_by_analysis(log, wave.wave))?;
        found.extend(unmet.into_iter().map(|item| (wave.wave, item)));
    }
    let mut asked: Vec<(u64, Unmet)> = Vec::new();
    for (wave, item) in found {
        let code = code_of(item.id);
        let born_of = (0..returned.len())
            .find(|at| returned[*at].0 == wave && listed_ids(&returned[*at].1, "origin").into_iter().any(|origin| code_of(origin) == code));
        match born_of {
            Some(at) => take_unmet(&mut returned[at].1, AnsweredBy::Wave(wave), &code, &item, lang),
            None => asked.push((wave, item)),
        }
    }
    let summaries: Vec<Summary> = waves
        .iter()
        .filter(|wave| !wave.undone.is_empty())
        .map(|wave| Summary { text: wave.delivered.clone(), tasks: wave.undone.iter().filter_map(|(id, _)| log.current(*id).map(|task| task.id)).collect() })
        .collect();
    Ok(place_unmet(log, asked, AnsweredBy::Wave, returned, &returning, &summaries, lang, ask))
}

/// O destino de cada item que a revisão final não cumpriu (`unmet`), pelo
/// classificador das atividades da spec `spec` de `root`
/// ([`task_triage::jev_ask`]), contra as tarefas ainda por entregar, como o
/// da volta de uma onda ([`place_unmet`]), de autor da revisão. Devolve cada
/// evento que os itens viram, com o tipo, na ordem dos itens, e cada tarefa
/// que ganhou itens pelo sentido.
pub(super) fn place_review_unmet(root: &Path, spec: &str, log: &SpecLog, unmet: Vec<Unmet>, lang: Locale) -> (Vec<(&'static str, Map<String, Value>)>, UnmetJoins) {
    place_review_unmet_with(log, unmet, lang, &task_triage::jev_ask(root, spec))
}

/// [`place_review_unmet`] com a pergunta ao Jev feita por `ask`.
fn place_review_unmet_with(
    log: &SpecLog,
    unmet: Vec<Unmet>,
    lang: Locale,
    ask: &dyn Fn(&str) -> Option<Value>,
) -> (Vec<(&'static str, Map<String, Value>)>, UnmetJoins) {
    let asked = unmet.into_iter().map(|item| ((), item)).collect();
    let (events, joins) = place_unmet(log, asked, |()| AnsweredBy::Review, &mut [], &BTreeSet::new(), &[], lang, ask);
    (events.into_iter().map(|((), kind, draft)| (kind, draft)).collect(), joins)
}

/// O destino de cada item não cumprido de `asked`, cada um com a marca de
/// quem respondeu por ele, que `by` lê: os itens vão ao Jev num pedido só,
/// contra as tarefas ainda por entregar, fora as das ondas em `returning`, e
/// as versões devolvidas em `returned`, com os resumos `summaries`. O coberto
/// vira nota que aponta a tarefa ([`covered_note`]); o de mesmo sentido entra
/// na tarefa escolhida — na versão devolvida, quando é uma delas, ou numa
/// versão nova da tarefa do backlog, uma só por tarefa — e a junção volta
/// para a reescrita ([`JoinedAt`]); o novo vira tarefa nova ([`unmet_task`]),
/// no grupo da tarefa de assunto mais próximo ([`group_after`]). Sem leitura
/// confiante, o item da volta de uma onda entra na primeira tarefa que ela
/// devolveu e, sem ela, vira tarefa nova sem grupo, como o da revisão final.
#[allow(clippy::too_many_arguments)]
fn place_unmet<T: Copy>(
    log: &SpecLog,
    asked: Vec<(T, Unmet)>,
    by: impl Fn(T) -> AnsweredBy,
    returned: &mut [(u64, Map<String, Value>)],
    returning: &BTreeSet<u64>,
    summaries: &[Summary],
    lang: Locale,
    ask: &dyn Fn(&str) -> Option<Value>,
) -> (Vec<(T, &'static str, Map<String, Value>)>, UnmetJoins) {
    let codes = log.codes();
    let code_of = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
    let back: Vec<u64> = returned.iter().filter_map(|(_, task)| task.get("replaces").and_then(Value::as_u64)).collect();
    let left = backlog_left(log);
    let open: Vec<&SpecEvent> = tasks_not_delivered(log, returning).into_iter().chain(back.iter().copied()).filter_map(|id| log.get(id)).collect();
    let candidates: Vec<Candidate> = open
        .iter()
        .map(|task| Candidate {
            id: task.id,
            title: task.str_field("title").unwrap_or_default(),
            text: task.str_field("text").unwrap_or_default(),
            files: task_files(task),
            joinable: left.contains(&task.id) || back.contains(&task.id),
        })
        .collect();
    let path_of = |file: &Value| file.get("path").and_then(Value::as_str).map(str::to_string);
    let items: Vec<Item> = asked
        .iter()
        .map(|(_, item)| Item {
            title: log.get(item.id).and_then(|event| event.str_field("title")).map_or_else(|| code_of(item.id), str::to_string),
            text: item.text.clone(),
            files: item.files.iter().filter_map(path_of).collect(),
        })
        .collect();
    let decided = task_triage::classify_with(&candidates, &items, summaries, ask);
    let mut events: Vec<(T, &'static str, Map<String, Value>)> = Vec::new();
    let mut joins: BTreeMap<JoinedAt, Vec<u64>> = BTreeMap::new();
    // A versão nova de cada tarefa do backlog que já recebeu um item, pelo
    // número dela, com a posição em `events`.
    let mut versions: BTreeMap<u64, usize> = BTreeMap::new();
    for ((tag, item), destination) in asked.into_iter().zip(decided) {
        let (answered, code) = (by(tag), code_of(item.id));
        if let Some(Destination::Covered { task, chance }) = destination
            && let Some(task) = log.get(task)
        {
            events.push((tag, "note", covered_note(log, (answered, &code, &item), task, chance, lang)));
            continue;
        }
        if let Some(Destination::Join { task, .. }) = destination {
            if let Some(at) = returned.iter().position(|(_, back)| back.get("replaces").and_then(Value::as_u64) == Some(task)) {
                take_unmet(&mut returned[at].1, answered, &code, &item, lang);
                joins.entry(JoinedAt::Returned(at)).or_default().push(item.id);
                continue;
            }
            if let Some(task) = log.get(task) {
                let at = *versions.entry(task.id).or_insert_with(|| {
                    events.push((tag, "task", backlog_return(task)));
                    events.len() - 1
                });
                take_unmet(&mut events[at].2, answered, &code, &item, lang);
                joins.entry(JoinedAt::Event(at)).or_default().push(item.id);
                continue;
            }
        }
        if let Some(Destination::New { near }) = destination {
            let mut task = unmet_task(item, answered.author());
            if let Some(group) = near.and_then(|near| group_after(log, near)) {
                task.insert(GROUP.into(), group);
            }
            events.push((tag, "task", task));
            continue;
        }
        let first = match answered {
            AnsweredBy::Wave(wave) => returned.iter().position(|(from, _)| *from == wave),
            AnsweredBy::Review => None,
        };
        match first {
            Some(first) => take_unmet(&mut returned[first].1, answered, &code, &item, lang),
            None => events.push((tag, "task", unmet_task(item, answered.author()))),
        }
    }
    (events, joins.into_iter().collect())
}

/// As junções pelo sentido `joins` como as das sobras ([`MeaningJoin`]),
/// pela posição entre as entradas que a rodada grava: a versão devolvida a
/// partir de `returned_from`, o evento a partir de `events_from`. Os itens
/// juntados já existem na spec e entram na reescrita pelo número.
pub(super) fn unmet_meaning_joins(joins: UnmetJoins, returned_from: usize, events_from: usize) -> Vec<MeaningJoin> {
    joins
        .into_iter()
        .map(|(at, items)| MeaningJoin {
            task: match at {
                JoinedAt::Returned(at) => returned_from + at,
                JoinedAt::Event(at) => events_from + at,
            },
            requests: Vec::new(),
            items,
        })
        .collect()
}

#[cfg(test)]
pub(super) mod tests {
    use std::path::Path;

    use mustard_core::io::spec_events as store;
    use mustard_core::io::wave_prompt::shown;
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::super::tests::{approved, approved_with, returned, round, waves_in, write};
    use super::{agreed_prompt, request_agreed};
    use crate::commands::spec_events::read::read_by_command;

    /// A spec `x` com a onda 1 sobre `src/app.ts` e duas regras do projeto
    /// todo; o Jev tira a segunda do pedido, e a onda sai. Devolve a resposta
    /// da rodada que a despachou.
    pub(in crate::commands::flow::round) fn sent_without_the_second_rule(root: &Path) -> Value {
        approved_with(root, "x", &[(1, &["src/app.ts"], &[])], |said| {
            for text in ["Vale sempre: a tabela nova tem chave.", "Vale sempre: a spec vira um PR só."] {
                let rule = json!({"title": text, "text": text, "example": "e", "keys": ["k"],
                    "applies_to": {"files": ["**"]}, "origin": said});
                assert_eq!(write(root, "x", "rule", rule)["ok"], json!(true));
            }
        });
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let second = log.codes().into_iter().find(|(_, code)| code == "MSTD-RULE-0002").map(|(id, _)| id).expect("the second rule");
        let _jev = super::super::item_choice::fake::answering(move |board| {
            board.items.iter().map(|item| (item.id, if item.id == second { 0.0 } else { 1.0 })).collect()
        });
        let sent = round(root, "x", None);
        assert_eq!(waves_in(&sent, "dispatch"), vec![1], "{sent}");
        sent
    }

    /// A spec `x` com a onda 1 sobre um arquivo em TypeScript e outro em C#;
    /// a onda volta sem cumprir a própria tarefa, sem citar arquivo na
    /// resposta, e a rodada assume a volta. Devolve o código da tarefa da
    /// onda 1.
    pub(in crate::commands::flow::round) fn returned_with_its_task_unmet(root: &Path) -> String {
        approved(root, "x", &[(1, &["src/app.ts", "src/Api.cs"], &[])]);
        let sent = round(root, "x", None);
        assert_eq!(waves_in(&sent, "dispatch"), vec![1], "{sent}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let task = log.visible().into_iter().find(|e| e.event_type == "task" && e.wave() == Some(1)).cloned().unwrap();
        let code = log.codes()[&task.id].clone();
        let unmet = json!([{"item": code, "met": false, "text": "Falta a segunda metade."}]);
        let wrote = returned(root, json!({"wave": 1, "text": "A onda 1 saiu pela metade.", "agreed": unmet}));
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let took = round(root, "x", None);
        assert_eq!(took["ok"], json!(true), "{took}");
        code
    }

    /// A tarefa que a rodada cria para refazer um item não cumprido nasce com
    /// os arquivos que o item declara, mesmo quando a resposta não cita
    /// nenhum: a onda que a recebe sai com o escopo do item.
    #[test]
    fn the_task_that_redoes_an_unmet_item_inherits_the_item_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let code = returned_with_its_task_unmet(root);

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let redoing: Vec<_> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "task" && e.ints("covers").iter().any(|id| codes.get(id) == Some(&code)))
            .collect();
        assert_eq!(redoing.len(), 1, "{redoing:?}");
        assert_eq!(redoing[0].str_field("text"), Some("Falta a segunda metade."), "{redoing:?}");
        let expected = json!([{"path": "src/app.ts"}, {"path": "src/Api.cs"}]);
        assert_eq!(redoing[0].fields["files"], expected, "{redoing:?}");
    }

    /// O comando que lê o bloco `block` da spec `x` do projeto em `root`.
    pub(in crate::commands::flow::round) fn read_block(root: &Path, block: &str) -> String {
        read_by_command(&format!("mustard-rt run read {block} --root {} --spec x", shown(root)))
    }

    /// O pedido mostra e a entrega cobra os mesmos itens combinados: os da
    /// lista que o envio da onda gravou. Depois de um reenvio gravado com a
    /// lista e o pedido do envio anterior, mas sem a escolha que tirou a
    /// segunda regra — como os reenvios ficaram gravados —, o pedido não a
    /// mostra, a entrega não é cobrada por ela e grava sem ela.
    #[test]
    fn the_request_and_its_delivery_carry_the_agreed_items_its_send_recorded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        sent_without_the_second_rule(root);
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let prior = log.visible().into_iter().rfind(|e| e.event_type == "send" && e.wave() == Some(1)).cloned().unwrap();
        assert!(prior.fields.get("analysis").is_some(), "{prior:?}");
        let mut resent = serde_json::Map::new();
        for field in ["wave", "role", "text", "lines", "chars", "items", "read_items", "mustard", "copy", "author"] {
            if let Some(value) = prior.fields.get(field) {
                resent.insert(field.into(), value.clone());
            }
        }
        resent.insert("resends".into(), json!(prior.id));
        store::write(&path, "send", resent, &[]).unwrap();

        let log = store::read(&path).unwrap().unwrap();
        let codes = log.codes();
        let charged: Vec<String> = request_agreed(&log, 1).iter().map(|item| codes[&item.id].clone()).collect();
        assert_eq!(charged, ["MSTD-RULE-0001"], "only what the request carried");
        let request = read_block(root, "request-1");
        for item in agreed_prompt::all_agreed(&log) {
            let code = &codes[&item.id];
            assert_eq!(request.contains(code.as_str()), charged.contains(code), "{code}, side by side: {request}");
        }
        let wrote = returned(root, json!({"wave": 1, "text": "A onda 1 saiu.", "agreed": [{"item": "MSTD-RULE-0001", "met": true}]}));
        assert_eq!(wrote["ok"], json!(true), "the removed rule is not charged: {wrote}");
    }

    /// A resposta confiante do Jev com a opção `choice`.
    fn sure(choice: &str) -> Value {
        json!({ "choice": choice, "confidence": 0.95, "probabilities": { choice: 0.93, "runner-up": 0.04 } })
    }

    /// No backlog, a tarefa X muda `src/cart.ts`, e a Y, `src/price.ts`; a
    /// onda 1 devolve a tarefa T, nascida do quarto pedido. A volta não cumpre
    /// quatro pedidos: o quarto entra em T sem ir ao Jev; os outros três vão
    /// num pedido só. O primeiro, que X já cobre, vira nota que aponta X, com
    /// o motivo; o segundo, de mesmo sentido de Y, entra numa versão nova de
    /// Y, com o arquivo dele; o terceiro, novo, vira tarefa no grupo de Y.
    /// Nenhum dos três vai a T pela regra nativa. Sem Jev, os três vão a T,
    /// a primeira tarefa que a volta devolveu, e nada mais nasce, nem pela
    /// onda 2, que na mesma rodada deixa de cumprir o terceiro de novo.
    #[test]
    fn task_triage_places_each_unmet_item_by_meaning_not_by_the_returned_task() {
        use mustard_core::domain::spec_events::parse_log;
        use mustard_core::platform::i18n::{Locale, translate};

        use super::super::report::WaveReport;
        use super::join_unmet_with;

        let lines = [
            json!({"v":1,"id":1,"type":"task","title":"O carrinho perde o desconto","text":"Ao trocar a quantidade, o desconto some.",
                "files":[{"path":"src/cart.ts"}],"depends_on":[]}),
            json!({"v":1,"id":2,"type":"task","title":"O preço não arredonda","text":"O total mostra três casas.",
                "files":[{"path":"src/price.ts"}],"depends_on":[]}),
            json!({"v":1,"id":3,"type":"request","title":"Desconto fica com a quantidade","text":"Mudar a quantidade mantém o desconto.","effect":"new_waves"}),
            json!({"v":1,"id":4,"type":"request","title":"Total arredonda","text":"A soma mostra duas casas.","effect":"new_waves",
                "files":[{"path":"src/total.ts"}]}),
            json!({"v":1,"id":5,"type":"request","title":"Cupom sobrevive à recarga","text":"O cupom fica depois de recarregar.","effect":"new_waves"}),
            json!({"v":1,"id":6,"type":"request","title":"Frete grátis acima do mínimo","text":"O frete zera acima do mínimo.","effect":"new_waves"}),
            json!({"v":1,"id":7,"type":"task","wave":1,"title":"O frete","text":"Frete grátis.","files":[{"path":"src/shipping.ts"}],
                "depends_on":[],"origin":6}),
        ];
        let log = parse_log(&lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n"));
        let codes = log.codes();
        let unmet = |id: u64, text: &str| json!({"item": id, "met": false, "text": text});
        let report = |wave: u64, undone: Vec<(u64, String)>, agreed: Vec<Value>| WaveReport {
            wave,
            delivered: "Parei no frete.".into(),
            files: Vec::new(),
            commit: None,
            proofs: Vec::new(),
            fixes: Vec::new(),
            replan: None,
            undone,
            taken_elsewhere: false,
            leftovers: Vec::new(),
            agreed,
            returns: Vec::new(),
            usage: Default::default(),
        };
        let all = vec![unmet(3, "O desconto ainda some."), unmet(4, "Falta o arredondamento."), unmet(5, "O cupom some."), unmet(6, "Falta o mínimo.")];
        let wave = report(1, vec![(7, codes[&7].clone())], all);
        let back = || vec![(1, super::backlog_return(log.get(7).unwrap()))];
        let asked = std::cell::RefCell::new(Vec::<String>::new());
        let ask = |payload: &str| {
            asked.borrow_mut().push(payload.to_string());
            Some(json!({"answers": {"i0": sure("covered:t1"), "i1": sure("join:t2"), "i2": sure("new"), "i2_near": sure("t2")}}))
        };

        let mut returned = back();
        let (events, joins) = join_unmet_with(&log, std::slice::from_ref(&wave), &mut returned, Locale::PtBr, &ask).unwrap();
        let asked = asked.into_inner();
        assert_eq!(asked.len(), 1, "um pedido só ao Jev");
        let payload: Value = serde_json::from_str(&asked[0]).unwrap();
        assert!(payload["state"]["items"].get("i3").is_none(), "o item de que T nasceu não vai ao Jev: {payload}");
        assert_eq!(returned[0].1["covers"], json!([6]), "T recebe só o item de que nasceu: {:?}", returned[0].1);
        let (x, y) = (codes[&1].clone(), codes[&2].clone());
        let [note, version, born] = events.as_slice() else { panic!("{events:?}") };
        assert_eq!((note.1, note.2["label"].clone()), ("note", json!(x)), "{note:?}");
        assert!(note.2["agent"].as_str().is_some_and(|reason| reason.contains("0.93")), "{note:?}");
        assert_eq!((version.1, version.2["replaces"].clone(), version.2["covers"].clone()), ("task", json!(2), json!([4])), "{version:?}");
        assert_eq!(version.2["files"], json!([{"path": "src/price.ts"}, {"path": "src/total.ts"}]));
        let line = translate("round.unmet_joined", Locale::PtBr).replace("{wave}", "1").replace("{code}", &codes[&4]).replace("{text}", "Falta o arredondamento.");
        assert_eq!(version.2["agent"], json!(line));
        assert_eq!((born.1, born.2["covers"].clone(), born.2["group"].clone()), ("task", json!([5]), json!({"name": y, "position": 2})), "{born:?}");
        assert_eq!(joins, [(super::JoinedAt::Event(1), vec![4])], "só a junção pelo sentido volta para a reescrita");

        // Gravados do número 10 em diante, só a versão nova de Y leva o pedido
        // de reescrita, com Y e o item que já existe como fontes.
        let dir = tempdir().unwrap();
        let recorded = events.iter().enumerate().map(|(at, (_, kind, draft))| {
            let mut line = draft.clone();
            line.extend([("v".into(), json!(1)), ("id".into(), json!(10 + at)), ("type".into(), json!(kind))]);
            Value::Object(line)
        });
        let after = parse_log(&lines.iter().cloned().chain(recorded).map(|line| line.to_string()).collect::<Vec<_>>().join("\n"));
        let mut entries: Vec<Value> = events.iter().enumerate().map(|(at, (_, kind, _))| json!({"type": kind, "id": 10 + at})).collect();
        super::super::leftovers::ask_rewrites(dir.path(), &after, &super::unmet_meaning_joins(joins, 0, 0), &mut entries);
        let rewrite = &entries[1]["rewrite"];
        assert_eq!(rewrite["replaces"], json!(11), "{rewrite}");
        let prompt = rewrite["delegation"]["request"]["prompt"].as_str().unwrap_or_default();
        assert!(prompt.contains("O preço não arredonda") && prompt.contains("Total arredonda"), "{prompt}");
        assert!(entries[0].get("rewrite").is_none() && entries[2].get("rewrite").is_none(), "{entries:?}");

        let mut returned = back();
        let again = report(2, Vec::new(), vec![unmet(5, "O cupom some de novo.")]);
        let (events, joins) = join_unmet_with(&log, &[wave, again], &mut returned, Locale::PtBr, &|_| None).unwrap();
        assert!(events.is_empty() && joins.is_empty(), "sem Jev, nada nasce fora de T, nem pela segunda volta do mesmo item: {events:?}");
        assert_eq!(returned[0].1["covers"], json!([6, 3, 4, 5]), "{:?}", returned[0].1);
    }

    /// A revisão final não cumpre três pedidos, que vão ao Jev num pedido só,
    /// contra as tarefas X e Y do backlog: o primeiro, que X cobre, vira nota
    /// que aponta X, de autor da revisão; o segundo entra numa versão nova de
    /// Y, com a linha da revisão final, e volta para a reescrita; o terceiro
    /// vira tarefa no grupo de Y. Sem Jev, os três viram tarefa nova, de
    /// autor da revisão, sem grupo.
    #[test]
    fn task_triage_places_each_unmet_item_of_the_final_review() {
        use mustard_core::domain::spec_events::parse_log;
        use mustard_core::platform::i18n::{Locale, translate};

        use super::{JoinedAt, Unmet, place_review_unmet_with};

        let lines = [
            json!({"v":1,"id":1,"type":"task","title":"O carrinho perde o desconto","text":"Ao trocar a quantidade, o desconto some.",
                "files":[{"path":"src/cart.ts"}],"depends_on":[]}),
            json!({"v":1,"id":2,"type":"task","title":"O preço não arredonda","text":"O total mostra três casas.",
                "files":[{"path":"src/price.ts"}],"depends_on":[]}),
            json!({"v":1,"id":3,"type":"request","title":"Desconto fica com a quantidade","text":"Mudar a quantidade mantém o desconto.","effect":"new_waves"}),
            json!({"v":1,"id":4,"type":"request","title":"Total arredonda","text":"A soma mostra duas casas.","effect":"new_waves"}),
            json!({"v":1,"id":5,"type":"request","title":"Cupom sobrevive à recarga","text":"O cupom fica depois de recarregar.","effect":"new_waves"}),
        ];
        let log = parse_log(&lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n"));
        let codes = log.codes();
        let unmet = || {
            [(3, "O desconto ainda some."), (4, "Falta o arredondamento."), (5, "O cupom some.")]
                .map(|(id, text)| Unmet { id, text: text.into(), files: vec![json!({"path": "src/total.ts"})] })
                .into()
        };
        let ask = |_: &str| Some(json!({"answers": {"i0": sure("covered:t1"), "i1": sure("join:t2"), "i2": sure("new"), "i2_near": sure("t2")}}));

        let (events, joins) = place_review_unmet_with(&log, unmet(), Locale::PtBr, &ask);
        let [note, version, born] = events.as_slice() else { panic!("{events:?}") };
        let covered = translate("round.unmet_review_covered", Locale::PtBr)
            .replace("{code}", &codes[&3])
            .replace("{task}", &codes[&1])
            .replace("{text}", "O desconto ainda some.");
        assert_eq!((note.0, &note.1["text"], &note.1["author"]), ("note", &json!(covered), &json!("review")), "{note:?}");
        let line = translate("round.unmet_review_joined", Locale::PtBr).replace("{code}", &codes[&4]).replace("{text}", "Falta o arredondamento.");
        assert_eq!((version.0, &version.1["replaces"], &version.1["covers"], &version.1["agent"]), ("task", &json!(2), &json!([4]), &json!(line)));
        assert_eq!((born.0, &born.1["covers"], &born.1["author"]), ("task", &json!([5]), &json!("review")), "{born:?}");
        assert_eq!(born.1["group"], json!({"name": codes[&2], "position": 2}));
        assert_eq!(joins, [(JoinedAt::Event(1), vec![4])]);

        let (events, joins) = place_review_unmet_with(&log, unmet(), Locale::PtBr, &|_| None);
        let shapes: Vec<(&str, &Value, &Value, bool)> =
            events.iter().map(|(kind, draft)| (*kind, &draft["covers"], &draft["author"], draft.contains_key("group"))).collect();
        assert_eq!(shapes, [("task", &json!([3]), &json!("review"), false), ("task", &json!([4]), &json!("review"), false), ("task", &json!([5]), &json!("review"), false)]);
        assert!(joins.is_empty());
    }
}
