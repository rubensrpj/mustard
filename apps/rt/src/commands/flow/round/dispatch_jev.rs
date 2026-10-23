//! As perguntas ao Jev do despacho — a escolha das skills das tarefas, a
//! formação e a ordem da lista, a montagem das ondas e a escolha dos itens do
//! pedido — saem antes de a rodada pegar a trava das cópias e a do passo do
//! git. A passada de antes ([`ask_before_locks`]) refaz, pela spec gravada, o
//! despacho que a rodada vai fazer, sem gravar onda nenhuma, pergunta ao Jev o
//! que ainda não tem decisão, pelo prazo de toda chamada (10 s), e grava a
//! resposta de cada pedido pela impressão dele, como etapa da rodada. Com as
//! travas presas, o despacho só lê ([`Phase::Read`]): a decisão gravada. Sem
//! ela, com o Jev ligado, só a onda ou a tarefa que depende dela espera nesta
//! rodada, e a pergunta sai na passada de antes da rodada seguinte.
//!
//! O Jev que falha, passa do prazo ou volta ilegível manda a pergunta à
//! reserva (o modelo de análise da sessão), em `reserves`; a passada seguinte
//! lê o recibo dela, grava-o como a decisão e só pergunta ao Jev de novo
//! enquanto ele não volta. A ordem da lista não vai a esta reserva: a leitura
//! do modelo pequeno já responde as perguntas dela. Com o Jev desligado, vale
//! a regra de sempre, sem espera.
//!
//! A impressão de cada pedido deixa de fora o número das tarefas, que muda a
//! cada versão: a onda montada que esperou e volta a ser montada, com as
//! mesmas tarefas, acha a mesma decisão, e a espera não se repete. Cada pedido
//! fica gravado por si: a decisão de uma tarefa vale em qualquer montagem que
//! a leve, e a rodada em que a spec mudou entre a pergunta e a trava só espera
//! pelo pedido que mudou. A onda montada que a escolha dos itens segurou não
//! volta a ser montada: a montagem a mantém enquanto a decisão dela falta ou
//! veio depois dela ([`Asker::undecided_at`]), e ela sai com o número que
//! ganhou.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::platform::i18n::Locale;
use serde_json::{Map, Value, json};

use crate::commands::spec_events::write::record;
use crate::shared::jev::reserve;
use crate::shared::judgement::{profile_aliases, remap_answers};

/// A etapa da rodada com que as respostas do despacho ficam gravadas, seguida
/// de `:` e do nome da chamada.
const STAGE: &str = "jev-dispatch";

/// O que o despacho faz com a pergunta que ainda não tem decisão.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Antes das travas: pergunta ao Jev, ou lê o recibo da reserva, e grava.
    Ask,
    /// Com as travas presas: só lê o que está gravado.
    Read,
}

/// O que um ponto do despacho tem de decisão para os pedidos dele.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Outcome {
    /// A resposta de cada pedido, na ordem, na forma da do Jev.
    Answered(Vec<Value>),
    /// Vale o caminho sem Jev do ponto: o Jev desligado, ou a reserva que
    /// não se preparou.
    Rule,
    /// O ponto espera a decisão: a gravada que falta, com as travas presas, ou
    /// o recibo da reserva, antes delas.
    Waiting,
}

/// A pergunta ao Jev de um ponto: recebe os pedidos e devolve as respostas,
/// na ordem, ou `None` na falha.
pub(crate) type Invoke<'a> = dyn Fn(&[String]) -> Option<Vec<Value>> + 'a;

/// As decisões do despacho de uma rodada, gravadas na spec `spec` de `start`
/// (`root`, o repositório principal), e o que a fase dela faz com a que falta.
pub(crate) struct Asker<'a> {
    start: &'a Path,
    root: &'a Path,
    spec: &'a str,
    phase: Phase,
    known: RefCell<BTreeMap<String, Value>>,
    /// O número do evento que gravou primeiro a decisão de cada impressão.
    since: BTreeMap<String, u64>,
    reserves: RefCell<Vec<Value>>,
}

impl<'a> Asker<'a> {
    /// As decisões gravadas na spec lida em `log`, na fase `phase`.
    pub(crate) fn new(start: &'a Path, root: &'a Path, spec: &'a str, log: &SpecLog, phase: Phase) -> Self {
        let mut known = BTreeMap::new();
        let mut since = BTreeMap::new();
        let prefix = format!("{STAGE}:");
        for event in log.events.iter().filter(|e| e.event_type == "stage_run" && e.str_field("stage").is_some_and(|stage| stage.starts_with(&prefix))) {
            if let Some(kept) = event.str_field("output").and_then(|text| serde_json::from_str::<Map<String, Value>>(text).ok()) {
                for (print, doc) in kept {
                    since.entry(print.clone()).or_insert(event.id);
                    known.insert(print, doc);
                }
            }
        }
        Self { start, root, spec, phase, known: RefCell::new(known), since, reserves: RefCell::new(Vec::new()) }
    }

    /// `true` quando algum pedido de `payloads` da chamada `command` ainda não
    /// tinha decisão gravada no evento `formed`: a decisão falta, ou foi
    /// gravada depois dele.
    pub(crate) fn undecided_at(&self, command: &str, payloads: &[String], formed: u64) -> bool {
        payloads.iter().any(|payload| self.since.get(&print(command, payload).0).is_none_or(|at| *at > formed))
    }

    /// `true` antes das travas, quando a pergunta pode sair.
    pub(crate) fn asks(&self) -> bool {
        self.phase == Phase::Ask
    }

    /// A decisão gravada de cada pedido de `payloads` da chamada `command`,
    /// na ordem: a resposta, o nulo da regra de sempre, ou `None` sem decisão.
    pub(crate) fn lookup(&self, command: &str, payloads: &[String]) -> Vec<Option<Value>> {
        let known = self.known.borrow();
        payloads
            .iter()
            .map(|payload| {
                let (print, aliases) = print(command, payload);
                let mut doc = known.get(&print)?.clone();
                remap_answers(&mut doc, &aliases, true);
                Some(doc)
            })
            .collect()
    }

    /// Os pedidos de `payloads` sem decisão gravada.
    pub(crate) fn missing(&self, command: &str, payloads: &[String]) -> Vec<String> {
        self.lookup(command, payloads).into_iter().zip(payloads).filter(|(doc, _)| doc.is_none()).map(|(_, payload)| payload.clone()).collect()
    }

    /// Antes das travas: o recibo da reserva já entregue dos pedidos sem
    /// decisão de `payloads` vira a decisão deles. `true` quando gravou.
    pub(crate) fn take_receipt(&self, command: &str, payloads: &[String]) -> bool {
        let wanted = self.missing(command, payloads);
        if !self.asks() || wanted.is_empty() {
            return false;
        }
        let Some(docs) = reserve::receipt_for(self.root, &canonicals(&wanted)).filter(|docs| docs.len() == wanted.len()) else {
            return false;
        };
        self.record(command, &wanted, &docs, Instant::now());
        true
    }

    /// Antes das travas: as respostas `got` do Jev aos pedidos `wanted` da
    /// chamada `command`, feita em `started`. A legível fica gravada; a que
    /// falta ou não se lê vai à reserva, com `reserve`, e o ponto espera o
    /// recibo; a reserva que não se prepara grava a regra de sempre.
    pub(crate) fn settle(&self, command: &str, wanted: &[String], got: Option<Vec<Value>>, reserve: bool, started: Instant) -> Outcome {
        if wanted.is_empty() {
            return Outcome::Answered(Vec::new());
        }
        if let Some(docs) = got.filter(|docs| docs.len() == wanted.len() && wanted.iter().zip(docs).all(|(payload, doc)| readable(payload, doc))) {
            self.record(command, wanted, &docs, started);
            return Outcome::Answered(docs);
        }
        if !reserve {
            return Outcome::Waiting;
        }
        let request = reserve::reserve_request(self.root, self.spec, command, &canonicals(wanted));
        if request.get("unavailable").is_none() {
            self.reserves.borrow_mut().push(request);
            return Outcome::Waiting;
        }
        self.record(command, wanted, &vec![Value::Null; wanted.len()], started);
        Outcome::Rule
    }

    /// A decisão dos pedidos `payloads` da chamada `command`, com o Jev
    /// `invoke` (desligado, sem ele): a gravada; antes das travas, o recibo da
    /// reserva ou a resposta do Jev aos que faltam, gravada ([`Self::settle`]).
    pub(crate) fn answers(&self, command: &str, payloads: &[String], reserve: bool, invoke: Option<&Invoke<'_>>) -> Outcome {
        let Some(invoke) = invoke else { return Outcome::Rule };
        if self.asks() {
            let wanted = self.missing(command, payloads);
            let received = reserve && self.take_receipt(command, payloads);
            if !wanted.is_empty() && !received {
                let started = Instant::now();
                self.settle(command, &wanted, invoke(&wanted), reserve, started);
            }
        }
        self.decided(command, payloads).unwrap_or(Outcome::Waiting)
    }

    /// A decisão já gravada de todos os pedidos `payloads`: a regra, quando
    /// algum a gravou, ou as respostas; `None` com algum sem decisão.
    pub(crate) fn decided(&self, command: &str, payloads: &[String]) -> Option<Outcome> {
        let docs: Vec<Value> = self.lookup(command, payloads).into_iter().collect::<Option<_>>()?;
        Some(if docs.iter().any(Value::is_null) { Outcome::Rule } else { Outcome::Answered(docs) })
    }

    /// Os pedidos de reserva que a passada de antes deixou.
    pub(crate) fn into_reserves(self) -> Vec<Value> {
        self.reserves.into_inner()
    }

    /// Grava as respostas `docs` dos pedidos `payloads` da chamada `command`,
    /// feita em `started`, cada uma pela impressão do pedido dela, com só o
    /// que responde e a marca da reserva. Melhor esforço: a gravação que falha
    /// deixa o pedido sem decisão, e a passada seguinte pergunta de novo.
    fn record(&self, command: &str, payloads: &[String], docs: &[Value], started: Instant) {
        let mut output = Map::new();
        let mut prints = Vec::new();
        for (payload, doc) in payloads.iter().zip(docs) {
            let (print, aliases) = print(command, payload);
            let mut kept = if doc.is_null() { Value::Null } else { json!({ "answers": doc["answers"] }) };
            if let Some(mark) = doc.pointer("/_mustard").filter(|mark| mark.get("reserve").is_some()) {
                kept["_mustard"] = mark.clone();
            }
            remap_answers(&mut kept, &aliases, false);
            self.known.borrow_mut().insert(print.clone(), kept.clone());
            output.insert(print.clone(), kept);
            prints.push(print);
        }
        let fields = Map::from_iter([
            ("author".into(), json!("binary")),
            ("phase".into(), json!("delivery")),
            ("stage".into(), json!(format!("{STAGE}:{command}"))),
            ("result".into(), json!(if docs.iter().all(Value::is_null) { "skipped" } else { "pass" })),
            ("ms".into(), json!(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX))),
            ("fingerprint".into(), json!(reserve::subject(&prints))),
            ("output".into(), json!(Value::Object(output).to_string())),
        ]);
        let _ = record(self.start, self.spec, "stage_run", fields, PhaseWriter::Binary);
    }
}

/// `true` quando alguma resposta de `docs` veio da reserva com uma pergunta
/// incerta: o ponto que a lê cai no caminho sem Jev dele.
pub(crate) fn unsure(docs: &[Value]) -> bool {
    docs.iter().any(|doc| doc.pointer("/_mustard/unsure") == Some(&json!(true)))
}

/// `true` quando alguma resposta de `docs` veio da reserva.
pub(crate) fn reserved(docs: &[Value]) -> bool {
    docs.iter().any(|doc| doc.pointer("/_mustard/reserve").is_some())
}

/// O pedido `payload` sem o número das tarefas, e os nomes trocados nas
/// respostas para achar a mesma decisão: a tarefa sozinha da montagem vira
/// `t0`, e as tarefas da onda, na escolha dos itens, valem pelo que dizem, na
/// ordem do texto delas. É o pedido que a reserva recebe, e o recibo dela se
/// acha de novo pela mesma forma.
fn canonical(payload: &str) -> (String, BTreeMap<String, String>) {
    let (canonical, aliases) = profile_aliases(payload);
    let canonical = match serde_json::from_str::<Value>(&canonical) {
        Ok(mut body) if body.pointer("/state/task").is_some_and(Value::is_object) => {
            let mut shown: Vec<Value> = body["state"]["task"].as_object().into_iter().flatten().map(|(_, task)| task.clone()).collect();
            shown.sort_by_key(Value::to_string);
            body["state"]["task"] = Value::Object(shown.into_iter().enumerate().map(|(at, task)| (format!("t{}", at + 1), task)).collect());
            body.to_string()
        }
        _ => canonical,
    };
    (canonical, aliases)
}

/// Os pedidos `payloads` na forma de [`canonical`].
fn canonicals(payloads: &[String]) -> Vec<String> {
    payloads.iter().map(|payload| canonical(payload).0).collect()
}

/// A impressão do pedido `payload` da chamada `command`, pela forma de
/// [`canonical`], e os nomes trocados nas respostas. Leva o serviço e o
/// modelo do Jev, como o cache das respostas dele: outro Jev pergunta de
/// novo.
fn print(command: &str, payload: &str) -> (String, BTreeMap<String, String>) {
    let (canonical, aliases) = canonical(payload);
    let provider = format!("{}:{}", crate::shared::jev::service_url(), crate::shared::jev::JEV_MODEL);
    (reserve::subject(&[provider, command.to_string(), canonical]), aliases)
}

/// `true` quando a resposta `doc` responde toda pergunta do pedido
/// `payload`: a chance de sim entre 0 e 1, uma das opções da escolha, ou a
/// nota.
fn readable(payload: &str, doc: &Value) -> bool {
    let Ok(body) = serde_json::from_str::<Value>(payload) else {
        return false;
    };
    body["questions"].as_object().is_some_and(|questions| {
        questions.iter().all(|(key, question)| {
            let answer = &doc["answers"][key];
            match question["type"].as_str() {
                Some("noul") => answer["noul"].as_f64().is_some_and(|chance| (0.0..=1.0).contains(&chance)),
                Some("choice") => answer["choice"].as_str().is_some_and(|choice| question["criteria"].get(choice).is_some()),
                Some("score") => answer["score"].as_f64().is_some(),
                _ => false,
            }
        })
    })
}

/// A frase da onda `wave` que espera a decisão do Jev do ponto `key` do
/// despacho, no idioma `lang`.
pub(crate) fn waiting_line(key: &str, wave: u64, lang: Locale) -> String {
    mustard_core::platform::i18n::translate(key, lang).replace("{wave}", &wave.to_string())
}

/// As ondas de `ready` sem as de `waiting`.
pub(crate) fn without(ready: Vec<u64>, waiting: &BTreeSet<u64>) -> Vec<u64> {
    ready.into_iter().filter(|wave| !waiting.contains(wave)).collect()
}

/// A passada de antes das travas ([`ask_before_locks`]) de novo, depois do
/// commit que esta rodada acabou de fazer (`committed`), pela spec gravada
/// em `path`: a tarefa que esperava a onda comitada só fica pronta agora, e a
/// onda dela nasce na passada travada desta mesma rodada. Sem a pergunta
/// feita aqui, a escolha dos itens dela esperaria a decisão do Jev, e a onda
/// ficaria gravada sem sair. A trava das cópias segue presa: nenhuma outra
/// rodada solta onda no meio, e a pergunta a segura no máximo pelo prazo do
/// Jev; a do passo do git o commit já soltou. Devolve os pedidos de reserva
/// de `early`, os da passada de antes, mais os novos desta; sem commit,
/// `early` como veio.
pub(super) fn ask_after_commit(
    (start, root, spec, path): (&Path, &Path, &str, &Path),
    committed: bool,
    mut early: Vec<Value>,
    session: Option<&str>,
    lang: Locale,
) -> Vec<Value> {
    let Some(log) = mustard_core::io::spec_events::read(path).ok().flatten().filter(|_| committed) else { return early };
    for asked in ask_before_locks(start, root, spec, &log, session, lang) {
        if !early.contains(&asked) {
            early.push(asked);
        }
    }
    early
}

/// A passada antes das travas da rodada da spec `spec` de `start` (`root`,
/// o repositório principal), lida em `log`, na sessão `session` e no idioma
/// `lang`: refaz o despacho que a rodada vai fazer, sem gravar onda nenhuma —
/// a formação da lista, a montagem das ondas, a escolha dos itens de cada
/// onda pronta e a das skills das tarefas dela —, e cada pergunta ao Jev sem
/// decisão sai agora, com a resposta gravada. Volta os pedidos de reserva da
/// pergunta que o Jev não respondeu.
pub(super) fn ask_before_locks(start: &Path, root: &Path, spec: &str, log: &SpecLog, session: Option<&str>, lang: Locale) -> Vec<Value> {
    // Com o Jev desligado em todos os pontos, não há o que perguntar: o
    // despacho segue a regra de sempre, sem a passada.
    let waves_jev = crate::shared::jev::for_waves(root, spec);
    let context_jev = crate::shared::jev::for_context(root, spec);
    let picks_jev = crate::shared::jev::yes_no::project_jev(root, spec, crate::shared::skill_cycle::PICK_CALL);
    let order_jev = crate::shared::jev::yes_no::orchestration_jev(root, spec, crate::shared::jev::TASK_TRIAGE_CALL).is_some();
    if waves_jev.filter.is_none() && !super::item_choice::asks_jev(context_jev.filter.as_deref()) && picks_jev.is_none() && !order_jev {
        return Vec::new();
    }
    let asker = Asker::new(start, root, spec, log, Phase::Ask);
    let forming = crate::commands::flow::triage::form_for_round(start, spec, session, &asker);
    let judge = waves_jev.filter.as_deref().map(|jev| move |board: &crate::shared::judgement::Board| jev.judge_backlog(board));
    let judging = judge.as_ref().map(|judge| judge as &super::backlog::Judge<'_>);
    let limit = super::queue::max_parallel(root);
    let Ok(planned) = super::backlog::planned_backlog(start, spec, log, limit, (judging, &asker), forming.pending.is_some()) else {
        return asker.into_reserves();
    };
    let vacancies = super::vacancy::vacancies(root, spec, &planned);
    let stuck = super::waves_stuck(&planned);
    let ready = super::queue::next_waves(&planned, &vacancies, &stuck).unwrap_or_default();
    let (choices, waiting) = super::item_choice::choose_items(start, spec, &planned, &ready, context_jev.filter.as_deref(), &asker);
    let ready: BTreeSet<u64> = without(ready, &waiting).into_iter().collect();
    // As skills só se escolhem com o pedido montado: ele só se monta com o
    // Jev da escolha ligado.
    if let Some(jev) = picks_jev.filter(|_| !ready.is_empty()) {
        let flight = mustard_core::io::wave_prompt::Flight { running: ready.clone(), choices, ..Default::default() };
        let built = mustard_core::io::wave_prompt::prompts(root, spec, &planned, lang, &flight, &ready);
        crate::shared::skill_cycle::picks_decided(&built, &ready, Some(&*jev), &asker);
    }
    asker.into_reserves()
}
