//! A decisão do Jev que segura a volta de uma onda — o nome tirado citado e
//! a volta sem mudança — fora da trava do passo do git. Com a trava presa, a
//! rodada só lê a decisão já tomada ([`decided`]): a resposta do Jev que uma
//! rodada anterior gravou, ou o recibo da reserva dele. Sem ela, com o Jev
//! ligado, a volta fica fora do commit nesta rodada, e só ela ([`Waiting`]):
//! a pergunta sai depois de a trava soltar, pelo prazo de toda chamada
//! ([`ask_outside`]). A resposta fica gravada como etapa da rodada, achada
//! de novo pela chave da decisão — o pedido inteiro, ou, na triagem, a onda,
//! a pergunta e as atividades que ela decide —, e a rodada seguinte segura ou
//! comita por ela. O Jev que falha, passa do prazo ou volta ilegível manda a
//! pergunta à reserva, e a volta espera o recibo; até ele, a passagem
//! seguinte pergunta ao Jev de novo, com o mesmo pedido de reserva. Com o Jev
//! desligado, nada espera: vale a regra de sempre.
//!
//! O mesmo vale para a triagem das atividades que a rodada grava — o alvo da
//! mudança de plano, o item que a onda ou a revisão final não cumpriu e a
//! sobra ([`super::triage_questions`]): a pergunta só sai depois de soltas a
//! trava do passo do git e a das cópias. A resposta da triagem não se perde
//! quando a fila de tarefas muda no meio-tempo: as tarefas abertas são só as
//! opções do pedido, e a decisão é das atividades. O recibo da reserva também
//! não: o pedido de reserva fica gravado sob a chave da decisão, com o número
//! dele e a impressão do pedido de então, e o recibo se lê por eles com as
//! perguntas do pedido de agora ([`decided_by`]).

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use mustard_core::domain::spec_events::{SpecEvent, SpecLog};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use crate::commands::spec_events::write::record;
use crate::shared::jev::reserve::{self, Reserved};
use crate::shared::jev::yes_no::{self, Ask};

/// A etapa da rodada com que a resposta do Jev fica gravada, seguida de `:`
/// e do nome da chamada.
const STAGE: &str = "jev-decision";

/// A etapa da rodada com que o pedido de reserva da decisão fica gravado,
/// seguida de `:` e do nome da chamada.
const RESERVE_STAGE: &str = "jev-reserve";

/// A pergunta que segura a volta de uma onda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Question {
    /// O nome tirado citado é a peça que a onda tirou?
    RemovedName,
    /// O que o relato da volta sem mudança dá como motivo?
    NoChange,
    /// Qual tarefa não feita recebe a mudança de plano que não cita nenhuma?
    ChangeTarget,
    /// Para onde vai cada item combinado que a onda não cumpriu?
    UnmetItems,
    /// Para onde vai cada sobra da volta?
    Leftovers,
    /// Para onde vai cada item combinado que a revisão final não cumpriu?
    ReviewUnmet,
}

/// A volta da revisão final, que não é de onda nenhuma: as ondas contam de
/// 1, e a pergunta dela fica gravada sem onda.
pub(crate) const FINAL_REVIEW: u64 = 0;

impl Question {
    /// O nome com que a chamada ao Jev e a reserva ficam gravadas.
    pub(crate) fn command(self) -> &'static str {
        match self {
            Self::RemovedName => super::removed_check::CITED_CALL,
            Self::NoChange => crate::shared::jev::TRIAGE_CALL,
            Self::ChangeTarget | Self::UnmetItems | Self::Leftovers | Self::ReviewUnmet => crate::shared::jev::TASK_TRIAGE_CALL,
        }
    }

    /// `true` na pergunta do classificador das atividades
    /// ([`crate::shared::task_triage`]).
    pub(crate) fn is_triage(self) -> bool {
        self.command() == crate::shared::jev::TASK_TRIAGE_CALL
    }

    /// O Jev desta pergunta na spec `spec` de `root`: o do projeto, para o
    /// nome tirado, e o da orquestração, para as outras. `None` com ele
    /// desligado.
    fn jev<'a>(self, root: &'a Path, spec: &'a str) -> Option<Ask<'a>> {
        match self {
            Self::RemovedName => yes_no::project_jev(root, spec, self.command()),
            _ => yes_no::orchestration_jev(root, spec, self.command()),
        }
    }

    /// A frase da volta da onda `wave` que espera a decisão desta pergunta,
    /// no idioma `lang`.
    pub(crate) fn pending(self, wave: u64, lang: Locale) -> String {
        let key = match self {
            Self::RemovedName => "round.jev_pending.removed_name",
            Self::NoChange => "round.jev_pending.no_change",
            Self::ChangeTarget => "round.jev_pending.change_target",
            Self::UnmetItems => "round.jev_pending.unmet_items",
            Self::Leftovers => "round.jev_pending.leftovers",
            Self::ReviewUnmet => "round.jev_pending.review_unmet",
        };
        translate(key, lang).replace("{wave}", &wave.to_string())
    }

    /// A etapa com que a resposta desta pergunta fica gravada.
    fn stage(self) -> String {
        format!("{STAGE}:{}", self.command())
    }

    /// A etapa com que o pedido de reserva desta pergunta fica gravado.
    fn reserve_stage(self) -> String {
        format!("{RESERVE_STAGE}:{}", self.command())
    }

    /// O nome da pergunta na chave da decisão: as da triagem dividem a
    /// chamada ao Jev e a etapa, e é ele que as separa.
    fn name(self) -> &'static str {
        match self {
            Self::RemovedName => "removed-name",
            Self::NoChange => "no-change",
            Self::ChangeTarget => "change-target",
            Self::UnmetItems => "unmet-items",
            Self::Leftovers => "leftovers",
            Self::ReviewUnmet => "review-unmet",
        }
    }
}

/// A chave da decisão da pergunta de triagem `question` da volta `wave` sobre
/// as atividades de impressão `items`: não leva as tarefas abertas, que são só
/// as opções da resposta. A fila que muda não troca a chave, e a pergunta só
/// se refaz quando as atividades mudam.
pub(crate) fn triage_key(wave: u64, question: Question, items: &str) -> String {
    reserve::subject(&[wave.to_string(), question.name().to_string(), items.to_string()])
}

/// A volta da onda `wave` que espera a decisão da pergunta `question`, feita
/// pelos pedidos `payloads` e achada de novo pela chave `key`.
#[derive(Debug, Clone)]
pub(crate) struct Waiting {
    pub(crate) wave: u64,
    pub(crate) question: Question,
    pub(crate) payloads: Vec<String>,
    pub(crate) key: String,
}

impl Waiting {
    /// A espera cuja decisão se acha pela impressão dos próprios pedidos
    /// `payloads`.
    pub(crate) fn by_payloads(wave: u64, question: Question, payloads: Vec<String>) -> Self {
        let key = reserve::subject(&payloads);
        Self { wave, question, payloads, key }
    }
}

/// As perguntas com o Jev ligado, lidas antes de a trava do passo do git ser
/// presa: a leitura da chave pode perguntar ao git.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct JevOn {
    removed_name: bool,
    no_change: bool,
    task_triage: bool,
}

impl JevOn {
    /// As perguntas com o Jev ligado na spec `spec` de `root`.
    pub(crate) fn read(root: &Path, spec: &str) -> Self {
        Self {
            removed_name: Question::RemovedName.jev(root, spec).is_some(),
            no_change: Question::NoChange.jev(root, spec).is_some(),
            task_triage: Question::Leftovers.jev(root, spec).is_some(),
        }
    }

    /// `true` quando a pergunta `question` tem o Jev ligado.
    fn asks(self, question: Question) -> bool {
        match question {
            Question::RemovedName => self.removed_name,
            Question::NoChange => self.no_change,
            _ => self.task_triage,
        }
    }
}

/// As respostas já decididas das perguntas da triagem que uma rodada lê com a
/// trava presa ([`decided`]), cada uma pela impressão do pedido de agora, e
/// não do que a pergunta levou quando foi feita: quem grava a atividade acha a
/// dela refazendo o mesmo pedido.
#[derive(Debug, Default)]
pub(crate) struct Decisions(BTreeMap<String, Value>);

impl Decisions {
    /// Guarda a resposta `doc` do pedido `payload`.
    pub(crate) fn keep(&mut self, payload: &str, doc: Value) {
        self.0.insert(reserve::subject(&[payload.to_string()]), doc);
    }

    /// A resposta decidida do pedido `payload`; `None` sem decisão, e então
    /// vale a regra de sempre.
    pub(crate) fn answer(&self, payload: &str) -> Option<Value> {
        self.0.get(&reserve::subject(&[payload.to_string()])).cloned()
    }
}

/// O que a rodada faz com a pergunta, com a trava presa.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Judged {
    /// Sem decisão do Jev: vale a regra de sempre — o Jev desligado, ou a
    /// reserva que não se preparou.
    Rule,
    /// A decisão, na forma da resposta do Jev, uma por pedido.
    Answered(Vec<Value>),
    /// A volta espera a decisão, fora desta rodada.
    Waiting,
}

/// A decisão da pergunta `question` feita pelos pedidos `payloads`, achada
/// pela impressão deles ([`decided_by`]).
pub(crate) fn decided(root: &Path, log: &SpecLog, question: Question, payloads: &[String], on: JevOn) -> Judged {
    decided_by(root, log, question, &reserve::subject(payloads), payloads, on)
}

/// As etapas `stage` gravadas em `log` sob a chave `key`, da mais nova para a
/// mais velha.
fn recorded<'a>(log: &'a SpecLog, stage: &'a str, key: &'a str) -> impl Iterator<Item = &'a SpecEvent> {
    log.events.iter().rev().filter(move |e| e.event_type == "stage_run" && e.str_field("stage") == Some(stage) && e.str_field("fingerprint") == Some(key))
}

/// A decisão da pergunta `question` feita pelos pedidos `payloads`, na spec
/// de `root` lida em `log`: a resposta gravada sob a chave `key` ou o recibo
/// de uma reserva pedida sob ela ([`reserve_receipt`]); sem nenhum dos dois,
/// a espera com o Jev ligado (`on`) e a regra com ele desligado. Só lê:
/// nenhuma chamada sai daqui.
pub(crate) fn decided_by(root: &Path, log: &SpecLog, question: Question, key: &str, payloads: &[String], on: JevOn) -> Judged {
    let stage = question.stage();
    if let Some(event) = recorded(log, &stage, key).next() {
        let docs = event.str_field("output").and_then(|text| serde_json::from_str::<Vec<Value>>(text).ok());
        return docs.filter(|docs| docs.len() == payloads.len()).map_or(Judged::Rule, Judged::Answered);
    }
    if !on.asks(question) {
        return Judged::Rule;
    }
    reserve_receipt(root, log, question, key, payloads).map_or(Judged::Waiting, Judged::Answered)
}

/// O recibo de uma reserva pedida para a decisão de chave `key` da pergunta
/// `question`, gravada em `log` com o número do pedido e a impressão dele
/// ([`record_reserve`]), lido com as perguntas dos pedidos de agora
/// `payloads`: o da mais nova que já tem recibo. A tarefa gravada entre o
/// pedido da reserva e o recibo muda o pedido de agora, e não o recibo.
fn reserve_receipt(root: &Path, log: &SpecLog, question: Question, key: &str, payloads: &[String]) -> Option<Vec<Value>> {
    let stage = question.reserve_stage();
    recorded(log, &stage, key).filter_map(|event| serde_json::from_str::<Value>(event.str_field("output")?).ok()).find_map(|asked| {
        match reserve::reserved_as(root, asked["request_id"].as_str()?, asked["subject"].as_str()?, payloads) {
            Reserved::Ready(docs) => Some(docs).filter(|docs| docs.len() == payloads.len()),
            Reserved::Pending | Reserved::Stale => None,
        }
    })
}

/// Depois de soltas a trava do passo do git e a das cópias: as perguntas das
/// voltas de `waiting` vão ao Jev, as de cada chamada juntas, pelo prazo de
/// toda chamada, e a resposta legível de cada volta fica gravada na spec
/// `spec` de `start`, para a rodada seguinte. A que falha vai à reserva:
/// volta o pedido pronto ao agente de análise, gravado sob a chave da
/// decisão. A reserva que não se prepara grava a regra de sempre, e a volta
/// não espera mais. Com o Jev desligado nesse meio-tempo, nada sai, e a
/// rodada seguinte segue a regra.
pub(crate) fn ask_outside(start: &Path, root: &Path, spec: &str, waiting: &[Waiting]) -> Vec<Value> {
    let mut reserves = Vec::new();
    let mut calls: Vec<Question> = Vec::new();
    for one in waiting {
        if !calls.iter().any(|call| call.command() == one.question.command()) {
            calls.push(one.question);
        }
    }
    for question in calls {
        let asking: Vec<&Waiting> = waiting.iter().filter(|one| one.question.command() == question.command()).collect();
        let Some(jev) = question.jev(root, spec) else { continue };
        let payloads: Vec<String> = asking.iter().flat_map(|one| one.payloads.iter().cloned()).collect();
        let started = Instant::now();
        let docs = jev(&payloads).filter(|docs| docs.len() == payloads.len());
        let mut at = 0;
        for one in asking {
            let answered = docs.as_ref().map(|docs| &docs[at..at + one.payloads.len()]);
            at += one.payloads.len();
            match answered.filter(|docs| one.payloads.iter().zip(docs.iter()).all(|(payload, doc)| readable(payload, doc))) {
                Some(docs) => record_decision(start, spec, one, docs, started),
                None => {
                    let request = reserve::reserve_request(root, spec, one.question.command(), &one.payloads);
                    if request.get("unavailable").is_none() {
                        record_reserve(start, spec, one, &request, started);
                        reserves.push(request);
                    } else {
                        record_decision(start, spec, one, &[], started);
                    }
                }
            }
        }
    }
    reserves
}

/// `true` quando a resposta `doc` responde toda pergunta do pedido
/// `payload`: a chance de sim entre 0 e 1, ou uma das opções da escolha.
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
                _ => false,
            }
        })
    })
}

/// Grava, na spec `spec` de `start`, a decisão da volta `one`: as respostas
/// `docs`, só o que responde, achadas de novo pela chave dela;
/// sem respostas, a regra de sempre. Melhor esforço: a gravação que falha
/// deixa a volta esperando, e a rodada seguinte pergunta de novo.
fn record_decision(start: &Path, spec: &str, one: &Waiting, docs: &[Value], started: Instant) {
    let kept: Vec<Value> = docs.iter().map(|doc| json!({ "answers": doc["answers"] })).collect();
    let result = if docs.is_empty() { "skipped" } else { "pass" };
    record_stage(start, spec, one, &one.question.stage(), result, &Value::Array(kept), started);
}

/// Grava, na spec `spec` de `start`, o pedido de reserva `request` da volta
/// `one`, cujo Jev falhou: o número do pedido e a impressão dos pedidos dela
/// de agora, achados de novo pela chave dela, por onde a rodada seguinte lê o
/// recibo ([`reserve_receipt`]). Melhor esforço: a gravação que falha deixa a
/// volta esperando, e a rodada seguinte pergunta de novo.
fn record_reserve(start: &Path, spec: &str, one: &Waiting, request: &Value, started: Instant) {
    let asked = json!({ "request_id": request["request_id"], "subject": reserve::subject(&one.payloads) });
    record_stage(start, spec, one, &one.question.reserve_stage(), "fail", &asked, started);
}

/// Grava, na spec `spec` de `start`, a etapa `stage` da volta `one`, com o
/// resultado `result` e a saída `output`, sob a chave dela e medida desde
/// `started`.
fn record_stage(start: &Path, spec: &str, one: &Waiting, stage: &str, result: &str, output: &Value, started: Instant) {
    let fields = Map::from_iter([
        ("author".into(), json!("binary")),
        ("phase".into(), json!("delivery")),
        ("stage".into(), json!(stage)),
        ("result".into(), json!(result)),
        ("ms".into(), json!(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX))),
        ("fingerprint".into(), json!(one.key)),
        ("output".into(), json!(output.to_string())),
        ("waves".into(), json!(if one.wave == FINAL_REVIEW { Vec::new() } else { vec![one.wave] })),
    ]);
    let _ = record(start, spec, "stage_run", fields, PhaseWriter::Binary);
}
