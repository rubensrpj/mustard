//! `survey` — o levantamento de uma spec: a lista de pontos que o `grill`
//! monta e a leitura dos pontos que continuam abertos.
//!
//! A lista ([`build`]) junta, nesta ordem:
//! - as lacunas do tipo de trabalho ([`gaps`]): funcionalidade, defeito e
//!   refatoração, juntas sem repetir no pedido misto, cada uma no bloco dela;
//!   a lacuna que o mapa do projeto já responde vem com o fato e a fonte;
//! - as lições do banco que casam com o objetivo, pelo BM25 de
//!   `domain::search`, com o texto original, nunca o `search`;
//! - as specs anteriores que casam com o objetivo, pelo `search` do índice,
//!   com as regras, as decisões e os erros delas que casam;
//! - dentro desses pontos, nunca num ponto novo, até 3 lembretes: mensagens do
//!   usuário nas specs anteriores que nenhum registro aponta ([`reminders`]).
//!
//! Quem grava os pontos é o assistente, pelo `write`, sobre esta lista, que
//! não é gravada: a mesma entrada dá sempre a mesma lista. [`missing`] diz
//! quais itens ainda não têm ponto gravado, e [`open_points`] diz quais pontos
//! gravados continuam abertos, na ordem dos blocos.
//!
//! O pedido que cabe numa frase tem o levantamento condensado: todos os
//! pontos num bloco só ([`CONDENSED`]), mostrados de uma vez para um "sim".
//!
//! Depois de cada gravação, [`next_step`] diz o passo seguinte: gravar os
//! pontos das lacunas que ainda não têm, o próximo ponto aberto, a revisão do
//! bloco cujo último ponto fechou e, sem ponto aberto, o fim, com as
//! mensagens do usuário sem destino. [`leave_survey`] diz
//! se a spec pode passar para o plano.
//!
//! Função pura: sem disco e sem relógio. Quem lê o banco, o índice, as specs
//! anteriores e o mapa é o comando.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::domain::citation::cited_names;
use crate::domain::lessons;
use crate::domain::project_map::{importers, ProjectMap};
use crate::domain::search::{query_terms, SearchIndex, TOP};
use crate::domain::spec_events::{search_field, Hidden, Refusal, SpecEvent, SpecLog, WORK_KINDS};
use crate::domain::spec_index::{title_of, IndexLine};
use crate::domain::spec_state::{original_of, State};
use crate::domain::text::fold_accents;
use crate::platform::i18n::{translate, Locale};

/// O bloco único do levantamento condensado.
pub const CONDENSED: &str = "condensed";

/// Os blocos do levantamento, na ordem em que os pontos são apresentados.
pub const BLOCKS: &[&str] = &[
    "context",
    "defect",
    "refactor",
    "rules",
    "limits",
    "contracts",
    "errors",
    "edge_cases",
    "out_of_scope",
    "proof",
    "lessons",
    "prior_specs",
    "code",
    "outside_review",
];

/// Quantos lembretes o levantamento traz no total.
pub const MAX_REMINDERS: usize = 3;

/// O casamento forte: quantas raízes do objetivo, no mínimo, uma mensagem
/// antiga ou uma spec anterior precisa ter para entrar. Com uma raiz só,
/// qualquer palavra comum ("pasta", "spec") traria lembrete e spec anterior.
pub const STRONG_ROOTS: usize = 2;

/// Quantos fatos de uma spec anterior entram no ponto dela.
pub const PRIOR_FACTS: usize = 3;

/// Quantos nomes citados no objetivo, e quantas declarações de cada um, o
/// mapa preenche.
const MAP_NAMES: usize = 3;

/// Quantos arquivos que importam o declarado entram no fato.
const MAP_IMPORTERS: usize = 20;

/// Os tipos da spec anterior que viram fato do ponto dela.
const PRIOR_FACT_TYPES: &[&str] = &["rule", "decision", "error"];

/// A origem de cada ponto da lista, no campo `from`.
const FROM_GAP: &str = "gap";
const FROM_LESSON: &str = "lesson";
const FROM_PRIOR_SPEC: &str = "prior_spec";
const FROM_OUTSIDE_REVIEW: &str = "outside_review";

/// Uma lacuna do tipo de trabalho: o que o levantamento precisa responder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GapKey {
    WhoUses,
    ExternalDeps,
    Symptom,
    Reproduction,
    ExpectedVsActual,
    Cause,
    MeasuredReason,
    MustNotChange,
    RemovedAndUsers,
    Moves,
    Dependents,
    GreenOrder,
    BeforeAfter,
    Rules,
    Limits,
    Contracts,
    Errors,
    EdgeCases,
    OutOfScope,
    DoneProof,
}

impl GapKey {
    /// Todas as lacunas, na ordem dos blocos.
    pub const ALL: [Self; 20] = [
        Self::WhoUses,
        Self::ExternalDeps,
        Self::Symptom,
        Self::Reproduction,
        Self::ExpectedVsActual,
        Self::Cause,
        Self::MeasuredReason,
        Self::MustNotChange,
        Self::RemovedAndUsers,
        Self::Moves,
        Self::Dependents,
        Self::GreenOrder,
        Self::BeforeAfter,
        Self::Rules,
        Self::Limits,
        Self::Contracts,
        Self::Errors,
        Self::EdgeCases,
        Self::OutOfScope,
        Self::DoneProof,
    ];

    /// O nome estável da lacuna.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::WhoUses => "who_uses",
            Self::ExternalDeps => "external_deps",
            Self::Symptom => "symptom",
            Self::Reproduction => "reproduction",
            Self::ExpectedVsActual => "expected_vs_actual",
            Self::Cause => "cause",
            Self::MeasuredReason => "measured_reason",
            Self::MustNotChange => "must_not_change",
            Self::RemovedAndUsers => "removed_and_users",
            Self::Moves => "moves",
            Self::Dependents => "dependents",
            Self::GreenOrder => "green_order",
            Self::BeforeAfter => "before_after",
            Self::Rules => "rules",
            Self::Limits => "limits",
            Self::Contracts => "contracts",
            Self::Errors => "errors",
            Self::EdgeCases => "edge_cases",
            Self::OutOfScope => "out_of_scope",
            Self::DoneProof => "done_proof",
        }
    }

    /// A chave do rótulo no catálogo.
    #[must_use]
    pub fn label_key(self) -> &'static str {
        match self {
            Self::WhoUses => "survey.gap.who_uses",
            Self::ExternalDeps => "survey.gap.external_deps",
            Self::Symptom => "survey.gap.symptom",
            Self::Reproduction => "survey.gap.reproduction",
            Self::ExpectedVsActual => "survey.gap.expected_vs_actual",
            Self::Cause => "survey.gap.cause",
            Self::MeasuredReason => "survey.gap.measured_reason",
            Self::MustNotChange => "survey.gap.must_not_change",
            Self::RemovedAndUsers => "survey.gap.removed_and_users",
            Self::Moves => "survey.gap.moves",
            Self::Dependents => "survey.gap.dependents",
            Self::GreenOrder => "survey.gap.green_order",
            Self::BeforeAfter => "survey.gap.before_after",
            Self::Rules => "survey.gap.rules",
            Self::Limits => "survey.gap.limits",
            Self::Contracts => "survey.gap.contracts",
            Self::Errors => "survey.gap.errors",
            Self::EdgeCases => "survey.gap.edge_cases",
            Self::OutOfScope => "survey.gap.out_of_scope",
            Self::DoneProof => "survey.gap.done_proof",
        }
    }

    /// O rótulo da lacuna no idioma pedido: o `gap` do ponto.
    #[must_use]
    pub fn label(self, lang: Locale) -> &'static str {
        translate(self.label_key(), lang)
    }

    /// O bloco da lacuna.
    #[must_use]
    pub fn block(self) -> &'static str {
        match self {
            Self::WhoUses | Self::ExternalDeps => "context",
            Self::Symptom | Self::Reproduction | Self::ExpectedVsActual | Self::Cause => "defect",
            Self::MeasuredReason
            | Self::MustNotChange
            | Self::RemovedAndUsers
            | Self::Moves
            | Self::Dependents
            | Self::GreenOrder
            | Self::BeforeAfter => "refactor",
            Self::Rules => "rules",
            Self::Limits => "limits",
            Self::Contracts => "contracts",
            Self::Errors => "errors",
            Self::EdgeCases => "edge_cases",
            Self::OutOfScope => "out_of_scope",
            Self::DoneProof => "proof",
        }
    }

    /// O `gap` gravado num ponto é esta lacuna: o rótulo dela em qualquer um
    /// dos dois idiomas, ou o nome dela, sem contar maiúscula, acento e
    /// espaço repetido.
    #[must_use]
    pub fn matches(self, written: &str) -> bool {
        let written = plain(written);
        !written.is_empty()
            && (written == self.name()
                || [Locale::PtBr, Locale::EnUs].into_iter().any(|lang| plain(self.label(lang)) == written))
    }

    /// As lacunas de um tipo de trabalho; nenhuma para um tipo que não existe.
    fn of_kind(kind: &str) -> &'static [Self] {
        match kind.trim() {
            "feature" => &[
                Self::WhoUses,
                Self::ExternalDeps,
                Self::Rules,
                Self::Limits,
                Self::Contracts,
                Self::Errors,
                Self::EdgeCases,
                Self::OutOfScope,
                Self::DoneProof,
            ],
            "fix" => &[Self::Symptom, Self::Reproduction, Self::ExpectedVsActual, Self::Cause, Self::DoneProof],
            "refactor" => &[
                Self::MeasuredReason,
                Self::MustNotChange,
                Self::RemovedAndUsers,
                Self::Moves,
                Self::Dependents,
                Self::GreenOrder,
                Self::BeforeAfter,
                Self::OutOfScope,
                Self::DoneProof,
            ],
            _ => &[],
        }
    }
}

/// As lacunas dos tipos de trabalho `kinds`, juntas sem repetir, na ordem dos
/// blocos: no pedido misto, a prova de pronto e o que fica fora aparecem uma
/// vez só.
#[must_use]
pub fn gaps(kinds: &[&str]) -> Vec<GapKey> {
    let mut out: Vec<GapKey> = kinds.iter().flat_map(|kind| GapKey::of_kind(kind).iter().copied()).collect();
    out.sort();
    out.dedup();
    out
}

/// Os tipos de trabalho escritos em `raw`, separados por vírgula, sem
/// repetir e na ordem de [`WORK_KINDS`]. Vazio quando nada foi escrito.
///
/// # Errors
///
/// O primeiro que não é tipo de trabalho, como foi escrito.
pub fn parse_kinds(raw: &str) -> Result<Vec<&'static str>, String> {
    let mut asked = BTreeSet::new();
    for word in raw.split(',').map(str::trim).filter(|w| !w.is_empty()) {
        match WORK_KINDS.iter().find(|kind| **kind == word) {
            Some(kind) => {
                asked.insert(*kind);
            }
            None => return Err(word.to_string()),
        }
    }
    Ok(WORK_KINDS.iter().copied().filter(|kind| asked.contains(kind)).collect())
}

/// O tipo de trabalho gravado: o `work_type` visível mais novo.
#[must_use]
pub fn work_type(log: &SpecLog) -> Option<&SpecEvent> {
    log.visible().into_iter().filter(|e| e.event_type == "work_type").max_by_key(|e| e.id)
}

/// Os tipos de um `work_type`, na ordem de [`WORK_KINDS`].
#[must_use]
pub fn kinds_of(event: &SpecEvent) -> Vec<&'static str> {
    let listed: Vec<&str> = event
        .fields
        .get("kinds")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::trim).collect())
        .unwrap_or_default();
    WORK_KINDS.iter().copied().filter(|kind| listed.contains(kind)).collect()
}

/// A lacuna `key` já tem um ponto gravado, aberto ou fechado.
#[must_use]
pub fn gap_recorded(log: &SpecLog, key: GapKey) -> bool {
    log.visible().into_iter().any(|e| {
        e.event_type == "point"
            && e.str_field("from").map(str::trim) == Some(FROM_GAP)
            && e.str_field("gap").is_some_and(|gap| key.matches(gap))
    })
}

/// As lacunas do tipo de trabalho gravado que ainda não têm ponto. Sem tipo
/// gravado, nenhuma.
#[must_use]
pub fn missing_gaps(log: &SpecLog) -> Vec<GapKey> {
    let kinds = work_type(log).map(kinds_of).unwrap_or_default();
    gaps(&kinds).into_iter().filter(|key| !gap_recorded(log, *key)).collect()
}

/// A ordem de um bloco na apresentação: o condensado primeiro, depois os de
/// [`BLOCKS`], e o bloco desconhecido por último.
#[must_use]
pub fn block_rank(block: &str) -> usize {
    let block = block.trim();
    if block == CONDENSED {
        return 0;
    }
    BLOCKS.iter().position(|known| *known == block).map_or(BLOCKS.len() + 1, |i| i + 1)
}

/// Os pontos gravados que continuam abertos: os `point` visíveis com a
/// situação `open` que nenhum `point` visível fecha. O `closes` pode apontar
/// o número do ponto ou o de qualquer versão dele. Em ordem de bloco e, no
/// mesmo bloco, pela ordem em que o ponto nasceu.
#[must_use]
pub fn open_points(log: &SpecLog) -> Vec<&SpecEvent> {
    let (born, closed) = born_open(log);
    let mut open: Vec<&SpecEvent> = born.into_iter().filter(|p| !closed.contains(&original_of(log, p))).collect();
    open.sort_by_key(|p| (block_rank(p.str_field("block").unwrap_or_default()), original_of(log, p), p.id));
    open
}

/// Os pontos gravados abertos que um `point` visível já fechou, pela mesma
/// leitura de [`open_points`]: os dois juntos são todos os pontos que
/// nasceram abertos. O ponto fechado cujo texto foi apagado depois, com
/// `purge`, conta pelo fechamento dele, que é o que sobra na leitura. Em
/// ordem de número.
#[must_use]
pub fn closed_points(log: &SpecLog) -> Vec<&SpecEvent> {
    let (born, closed) = born_open(log);
    let alive: BTreeSet<u64> = born.iter().map(|p| original_of(log, p)).collect();
    let hidden = log.hidden();
    let purged: BTreeSet<u64> = log
        .events
        .iter()
        .filter(|e| matches!(hidden.get(&e.id), Some(Hidden::Purged { .. })))
        .map(|e| original_of(log, e))
        .collect();
    let mut stood_for = BTreeSet::new();
    let closings_left: Vec<&SpecEvent> = log
        .visible()
        .into_iter()
        .filter(|e| e.event_type == "point")
        .filter(|e| {
            let first = e.int("closes").and_then(|id| log.get(id)).map(|target| original_of(log, target));
            first.is_some_and(|first| !alive.contains(&first) && purged.contains(&first) && stood_for.insert(first))
        })
        .collect();
    let mut out: Vec<&SpecEvent> =
        born.into_iter().filter(|p| closed.contains(&original_of(log, p))).chain(closings_left).collect();
    out.sort_by_key(|p| p.id);
    out
}

/// Os `point` visíveis com a situação `open`, na versão vigente, e a
/// primeira versão de cada ponto que algum `point` visível fecha.
fn born_open(log: &SpecLog) -> (Vec<&SpecEvent>, BTreeSet<u64>) {
    let points: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "point").collect();
    let closed: BTreeSet<u64> = points
        .iter()
        .filter_map(|p| p.int("closes"))
        .filter_map(|id| log.get(id))
        .map(|target| original_of(log, target))
        .collect();
    let born = points.into_iter().filter(|p| p.str_field("status").map(str::trim) == Some("open")).collect();
    (born, closed)
}

/// Os pontos como uma recusa os lista: o código, o número e a lacuna de cada
/// um, separados por ponto e vírgula; `-` quando não há nenhum.
#[must_use]
pub fn describe(log: &SpecLog, points: &[&SpecEvent]) -> String {
    if points.is_empty() {
        return "-".to_string();
    }
    let codes = log.codes();
    points
        .iter()
        .map(|p| {
            let gap = p.str_field("gap").map(str::trim).unwrap_or_default();
            match codes.get(&p.id) {
                Some(code) => format!("{code} ({}): {gap}", p.id),
                None => format!("{}: {gap}", p.id),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// A recusa de uma mudança de fase com os pontos abertos `points`.
#[must_use]
pub fn open_refusal(spec: &str, log: &SpecLog, points: &[&SpecEvent]) -> Refusal {
    Refusal::SurveyOpen { spec: spec.trim().to_string(), count: points.len(), points: describe(log, points) }
}

/// O que ainda segura a spec no levantamento.
#[derive(Debug, Clone, PartialEq)]
pub enum SurveyGap<'a> {
    /// O tipo de trabalho não foi gravado: o `grill` não rodou.
    NotStarted,
    /// Lacunas do tipo de trabalho gravado ainda sem ponto, na ordem dos
    /// blocos.
    Unrecorded(Vec<GapKey>),
    /// Os pontos abertos, na ordem dos blocos.
    Open(Vec<&'a SpecEvent>),
}

impl SurveyGap<'_> {
    /// A recusa da passagem da spec `spec`, cujo arquivo é `log`.
    #[must_use]
    pub fn refusal(&self, spec: &str, log: &SpecLog) -> Refusal {
        match self {
            Self::NotStarted => Refusal::SurveyNotStarted { spec: spec.trim().to_string() },
            Self::Unrecorded(gaps) => Refusal::SurveyGapsUnrecorded { spec: spec.trim().to_string(), gaps: gaps.clone() },
            Self::Open(points) => open_refusal(spec, log, points),
        }
    }
}

/// O levantamento está feito, e a spec pode passar para o plano: o tipo de
/// trabalho gravado, um ponto para cada lacuna dele e nenhum ponto aberto.
///
/// # Errors
///
/// O que falta, na ordem em que o levantamento anda: o tipo de trabalho, as
/// lacunas sem ponto e os pontos abertos.
pub fn leave_survey(log: &SpecLog) -> Result<(), SurveyGap<'_>> {
    if work_type(log).is_none() {
        return Err(SurveyGap::NotStarted);
    }
    let unrecorded = missing_gaps(log);
    if !unrecorded.is_empty() {
        return Err(SurveyGap::Unrecorded(unrecorded));
    }
    let open = open_points(log);
    if open.is_empty() { Ok(()) } else { Err(SurveyGap::Open(open)) }
}

/// O levantamento é condensado: algum ponto visível está no bloco único.
#[must_use]
pub fn condensed(log: &SpecLog) -> bool {
    log.visible().iter().any(|e| e.event_type == "point" && e.str_field("block").map(str::trim) == Some(CONDENSED))
}

/// Um passo do levantamento, que o `write` devolve depois de uma gravação.
#[derive(Debug, Clone, PartialEq)]
pub enum SurveyStep<'a> {
    /// Lacunas do tipo de trabalho ainda sem ponto, na ordem dos blocos:
    /// antes de qualquer ponto ser apresentado, os pontos delas são gravados.
    Record(Vec<GapKey>),
    /// O próximo ponto aberto: o mesmo, enquanto ele não fecha.
    Point(&'a SpecEvent),
    /// A gravação fechou o último ponto aberto do bloco `block`: a revisão
    /// dele, com os pontos do bloco (`closed`) e os registros que os
    /// fechamentos apontam em `result` (`records`). Sem ponto aberto em bloco
    /// nenhum, a revisão oferece o revisor de fora (`outside_review`), uma vez:
    /// com algum ponto vindo dele, não oferece de novo.
    ReviewBlock { block: String, closed: Vec<u64>, records: Vec<u64>, outside_review: bool },
    /// Não sobra ponto aberto: as mensagens do usuário que nenhum registro
    /// aponta.
    Done { unrouted: Vec<&'a SpecEvent> },
}

/// Os passos do levantamento depois de uma gravação, lidos do arquivo antes
/// (`before`) e depois (`after`) dela, em ordem:
///
/// - nenhum fora do levantamento e do plano, e numa spec sem nenhum ponto e
///   sem lacuna por gravar (as specs antigas);
/// - a revisão do bloco, quando a gravação fechou o último ponto aberto de um
///   bloco que não é o do levantamento condensado; o revisor de fora só é
///   oferecido quando o levantamento acabou e nenhum ponto veio dele ainda;
/// - depois dela, ou sozinho: enquanto alguma lacuna do tipo de trabalho não
///   tem ponto (a lista do `grill` ainda sendo gravada, ou um ponto
///   esquecido), gravar os pontos que faltam; senão, o próximo ponto aberto,
///   o primeiro na ordem dos blocos; sem ponto aberto, o fim, com as
///   mensagens do usuário sem destino. O fim sai na gravação que fecha o
///   último ponto e, depois dela, em cada gravação que muda a lista dessas
///   mensagens.
#[must_use]
pub fn next_step<'a>(before: &SpecLog, after: &'a SpecLog) -> Vec<SurveyStep<'a>> {
    if !matches!(State::from_log(after).phase, Some("survey" | "plan")) {
        return Vec::new();
    }
    let unrecorded = missing_gaps(after);
    if unrecorded.is_empty() && !after.visible().iter().any(|e| e.event_type == "point") {
        return Vec::new();
    }
    let (was, now) = (open_points(before), open_points(after));
    let blocks = |open: &[&SpecEvent]| -> BTreeSet<String> {
        open.iter().map(|p| p.str_field("block").unwrap_or_default().trim().to_string()).collect()
    };
    let still = blocks(&now);
    let mut emptied: Vec<String> =
        blocks(&was).into_iter().filter(|block| block != CONDENSED && !still.contains(block)).collect();
    emptied.sort_by_key(|block| block_rank(block));
    let mut steps = Vec::new();
    if let Some(block) = emptied.into_iter().next() {
        let (closed, records) = block_review(after, &block);
        let over = now.is_empty() && unrecorded.is_empty();
        steps.push(SurveyStep::ReviewBlock { block, closed, records, outside_review: over && !outside_reviewed(after) });
    }
    if !unrecorded.is_empty() {
        steps.push(SurveyStep::Record(unrecorded));
        return steps;
    }
    match now.first() {
        Some(point) => steps.push(SurveyStep::Point(point)),
        None => {
            let unrouted = unrouted_messages(after);
            let changed = unrouted.iter().map(|m| m.id).ne(unrouted_messages(before).iter().map(|m| m.id));
            if !was.is_empty() || changed {
                steps.push(SurveyStep::Done { unrouted });
            }
        }
    }
    steps
}

/// Os pontos do bloco `block` que nasceram abertos e já fecharam, na versão
/// vigente, e os registros que os fechamentos deles apontam em `result`, sem
/// repetir, em ordem de número.
fn block_review(log: &SpecLog, block: &str) -> (Vec<u64>, Vec<u64>) {
    let points: Vec<&SpecEvent> =
        closed_points(log).into_iter().filter(|p| p.str_field("block").map(str::trim) == Some(block)).collect();
    let originals: BTreeSet<u64> = points.iter().map(|p| original_of(log, p)).collect();
    let records: BTreeSet<u64> = log
        .visible()
        .into_iter()
        .filter(|e| e.event_type == "point")
        .filter(|e| {
            e.int("closes").and_then(|id| log.get(id)).is_some_and(|target| originals.contains(&original_of(log, target)))
        })
        .flat_map(|e| e.ints("result"))
        .collect();
    (points.iter().map(|p| p.id).collect(), records.into_iter().collect())
}

/// Algum ponto visível veio do revisor de fora: ele já conferiu o
/// levantamento, e a revisão do último bloco não o oferece de novo.
fn outside_reviewed(log: &SpecLog) -> bool {
    log.visible()
        .iter()
        .any(|e| e.event_type == "point" && e.str_field("from").map(str::trim) == Some(FROM_OUTSIDE_REVIEW))
}

/// O objetivo da spec: o primeiro `context` gravado, na versão vigente dele.
/// O objetivo revisto continua sendo o objetivo; o tirado deixa o lugar para
/// o `context` seguinte. É a mesma leitura do objetivo do índice.
#[must_use]
pub fn goal(log: &SpecLog) -> Option<&SpecEvent> {
    log.events
        .iter()
        .filter(|e| e.event_type == "context" && e.int("replaces").is_none())
        .find_map(|e| log.current(e.id))
}

/// As mensagens do usuário, visíveis, que nenhum evento visível aponta em
/// `origin`: as que não viraram nenhum registro. Responder a uma mensagem não
/// é destino para ela.
#[must_use]
pub fn unrouted_messages(log: &SpecLog) -> Vec<&SpecEvent> {
    let visible = log.visible();
    let routed: BTreeSet<u64> = visible.iter().filter_map(|e| e.int("origin")).collect();
    visible
        .into_iter()
        .filter(|e| e.event_type == "message" && e.str_field("author").map(str::trim) == Some("user"))
        .filter(|e| !routed.contains(&e.id))
        .collect()
}

/// Um lembrete: uma mensagem antiga do usuário, sem registro, que casa com o
/// objetivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    /// A spec em que a mensagem está.
    pub spec: String,
    /// O número da mensagem naquela spec.
    pub message: u64,
    /// O texto da mensagem, como o usuário escreveu.
    pub text: String,
    /// A nota do BM25, ×1024.
    pub score: u64,
}

impl Reminder {
    /// O lembrete como o ponto o guarda.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut out = Map::new();
        out.insert("spec".into(), Value::from(self.spec.as_str()));
        out.insert("message".into(), Value::from(self.message));
        out.insert("text".into(), Value::from(self.text.as_str()));
        Value::Object(out)
    }
}

/// Os lembretes para o objetivo `goal`, tirados das specs anteriores `prior`
/// (o nome e o arquivo de eventos de cada uma): as mensagens do usuário que
/// não viraram registro ([`unrouted_messages`]) com casamento forte
/// ([`STRONG_ROOTS`]), as `max` de nota maior pelo BM25, somando as specs. No
/// empate, o nome da spec e depois o número da mensagem. As respostas do
/// assistente nunca entram.
#[must_use]
pub fn reminders<'a>(prior: impl IntoIterator<Item = (&'a str, &'a SpecLog)>, goal: &str, max: usize) -> Vec<Reminder> {
    let terms = query_terms(goal);
    if terms.is_empty() || max == 0 {
        return Vec::new();
    }
    let mut candidates: Vec<(&str, u64, &str, String)> = Vec::new();
    for (name, log) in prior {
        for message in unrouted_messages(log) {
            let Some(text) = message.str_field("text").map(str::trim).filter(|t| !t.is_empty()) else {
                continue;
            };
            let search = message.str_field("search").map_or_else(|| search_field(Some(text), &[]), str::to_string);
            candidates.push((name, message.id, text, search));
        }
    }
    let index = SearchIndex::build(candidates.iter().enumerate().map(|(i, c)| (doc_id(i), c.3.as_str())));
    let mut found: Vec<Reminder> = index
        .top(&terms, candidates.len())
        .into_iter()
        .filter_map(|hit| {
            let (spec, message, text, search) = candidates.get(doc_pos(hit.id))?;
            strong(&terms, search).then(|| Reminder {
                spec: (*spec).to_string(),
                message: *message,
                text: (*text).to_string(),
                score: hit.score,
            })
        })
        .collect();
    found.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.spec.cmp(&b.spec)).then(a.message.cmp(&b.message)));
    found.truncate(max);
    found
}

/// Um fato de um ponto da lista, com a fonte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub text: String,
    pub source: String,
}

/// Um item da lista que o `grill` monta: o ponto que o assistente grava.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposed {
    pub block: String,
    pub gap: String,
    /// De onde o ponto vem: `gap`, `lesson` ou `prior_spec`.
    pub from: &'static str,
    /// A lacuna, quando o ponto é de uma.
    pub key: Option<GapKey>,
    /// Os fatos que a lista já traz, cada um com a fonte.
    pub facts: Vec<Fact>,
    pub reminders: Vec<Reminder>,
}

impl Proposed {
    /// O ponto como o assistente o grava, com a mensagem de origem.
    #[must_use]
    pub fn to_value(&self, origin: Option<u64>) -> Value {
        let mut out = Map::new();
        out.insert("block".into(), Value::from(self.block.as_str()));
        out.insert("gap".into(), Value::from(self.gap.as_str()));
        out.insert("from".into(), Value::from(self.from));
        if let Some(origin) = origin {
            out.insert("origin".into(), Value::from(origin));
        }
        if !self.facts.is_empty() {
            let facts = self
                .facts
                .iter()
                .map(|fact| {
                    let mut item = Map::new();
                    item.insert("text".into(), Value::from(fact.text.as_str()));
                    item.insert("source".into(), Value::from(fact.source.as_str()));
                    Value::Object(item)
                })
                .collect();
            out.insert("facts".into(), Value::Array(facts));
        }
        if !self.reminders.is_empty() {
            out.insert("reminders".into(), Value::Array(self.reminders.iter().map(Reminder::to_value).collect()));
        }
        Value::Object(out)
    }

    /// O ponto gravado que registra este item: o mais novo dos pontos
    /// visíveis com a mesma origem e a mesma lacuna. A lacuna é reconhecida
    /// pelo rótulo em qualquer idioma ou pelo nome; o resto, pelo texto do
    /// `gap`.
    #[must_use]
    pub fn recorded_by<'a>(&self, log: &'a SpecLog) -> Option<&'a SpecEvent> {
        log.visible().into_iter().filter(|e| e.event_type == "point" && self.is(e)).max_by_key(|e| e.id)
    }

    /// O ponto que registra este item pela leitura única dos pontos, com a
    /// situação dele: entre os abertos ([`open_points`]), `open`, e entre os
    /// fechados ([`closed_points`]), `closed`, o que tem a mesma origem e a
    /// mesma lacuna. O ponto fechado conta pela versão que nasceu aberta,
    /// também quando o fechamento grava outro texto na lacuna.
    #[must_use]
    pub fn standing<'a>(&self, log: &'a SpecLog) -> Option<(&'a SpecEvent, &'static str)> {
        let open = open_points(log).into_iter().map(|p| (p, "open"));
        let closed = closed_points(log).into_iter().map(|p| (p, "closed"));
        open.chain(closed).find(|(point, _)| self.is(point))
    }

    fn is(&self, point: &SpecEvent) -> bool {
        if point.str_field("from").map(str::trim) != Some(self.from) {
            return false;
        }
        let Some(gap) = point.str_field("gap") else {
            return false;
        };
        match self.key {
            Some(key) => key.matches(gap),
            None => plain(gap) == plain(&self.gap),
        }
    }
}

/// Os itens da lista que ainda não têm ponto gravado, na ordem da lista.
#[must_use]
pub fn missing<'a>(log: &SpecLog, list: &'a [Proposed]) -> Vec<&'a Proposed> {
    list.iter().filter(|item| item.recorded_by(log).is_none()).collect()
}

/// O que a lista usa, já lido por quem chama.
pub struct Sources<'a> {
    /// Os tipos de trabalho.
    pub kinds: &'a [&'a str],
    /// O texto do objetivo.
    pub goal: &'a str,
    /// A spec do levantamento, que nunca é a própria spec anterior.
    pub current: &'a str,
    /// O banco de lições, quando existe.
    pub bank: Option<&'a SpecLog>,
    /// O caminho do banco de lições a partir da raiz do projeto, para a fonte
    /// do fato de cada lição.
    pub lessons_file: &'a str,
    /// As linhas de spec do índice.
    pub index: &'a [IndexLine],
    /// O arquivo de eventos de cada spec do projeto.
    pub prior: &'a [(String, SpecLog)],
    /// O mapa do projeto, quando existe.
    pub map: Option<&'a ProjectMap>,
    /// O levantamento condensado: todos os pontos num bloco só.
    pub condensed: bool,
    /// O idioma dos rótulos.
    pub lang: Locale,
}

/// A lista de pontos do levantamento: as lacunas do tipo, as lições que
/// casam, as specs anteriores que casam e, dentro desses pontos, os
/// lembretes. Cada lembrete vai para o ponto cujo texto casa melhor com ele;
/// sem casamento, para o primeiro. Nenhum ponto nasce para um lembrete.
#[must_use]
pub fn build(sources: &Sources<'_>) -> Vec<Proposed> {
    let mut list: Vec<Proposed> = gaps(sources.kinds)
        .into_iter()
        .map(|key| Proposed {
            block: key.block().to_string(),
            gap: key.label(sources.lang).to_string(),
            from: FROM_GAP,
            key: Some(key),
            facts: map_facts(key, sources),
            reminders: Vec::new(),
        })
        .collect();
    list.extend(lesson_points(sources));
    list.extend(prior_spec_points(sources));
    let prior = sources.prior.iter().filter(|(name, _)| name != sources.current).map(|(name, log)| (name.as_str(), log));
    place(&mut list, reminders(prior, sources.goal, MAX_REMINDERS));
    if sources.condensed {
        for item in &mut list {
            item.block = CONDENSED.to_string();
        }
    }
    list
}

/// Os fatos que o mapa do projeto já dá para a lacuna: para quem depende, na
/// refatoração, onde cada nome citado no objetivo é declarado e quem importa
/// esse arquivo.
fn map_facts(key: GapKey, sources: &Sources<'_>) -> Vec<Fact> {
    let Some(map) = sources.map.filter(|_| key == GapKey::Dependents) else {
        return Vec::new();
    };
    let mut facts: Vec<Fact> = Vec::new();
    let mut push = |fact: Fact| {
        if !facts.contains(&fact) {
            facts.push(fact);
        }
    };
    for name in cited_names(sources.goal).into_iter().take(MAP_NAMES) {
        for (path, line) in map.declared(&name).into_iter().take(MAP_NAMES) {
            push(Fact {
                text: translate("survey.fact_declared", sources.lang)
                    .replace("{name}", &name)
                    .replace("{path}", &path)
                    .replace("{line}", &line.to_string()),
                source: format!("{path}:{line}"),
            });
            let users = importers(map, &path).unwrap_or_default();
            if users.is_empty() {
                continue;
            }
            let mut listed = users.iter().take(MAP_IMPORTERS).cloned().collect::<Vec<_>>().join(", ");
            if users.len() > MAP_IMPORTERS {
                listed.push_str(", …");
            }
            push(Fact {
                text: translate("survey.fact_importers", sources.lang)
                    .replace("{path}", &path)
                    .replace("{importers}", &listed),
                source: format!("mustard-rt run map importers --file {path}"),
            });
        }
    }
    facts
}

/// Um ponto por lição do banco que casa com o objetivo, as mais fortes
/// primeiro: o título da lição como lacuna e o texto original dela como fato,
/// com o arquivo e a linha do banco como fonte.
fn lesson_points(sources: &Sources<'_>) -> Vec<Proposed> {
    let Some(bank) = sources.bank else {
        return Vec::new();
    };
    lessons::matching(bank, sources.goal)
        .into_iter()
        .filter_map(|hit| {
            let lesson = bank.get(hit.id)?;
            let text = lesson.str_field("text").map(str::trim).filter(|t| !t.is_empty())?;
            Some(Proposed {
                block: "lessons".to_string(),
                gap: title_of(lesson).unwrap_or_else(|| text.to_string()),
                from: FROM_LESSON,
                key: None,
                facts: vec![Fact { text: text.to_string(), source: format!("{}:{}", sources.lessons_file, lesson.line) }],
                reminders: Vec::new(),
            })
        })
        .collect()
}

/// Um ponto por spec anterior que casa com o objetivo pelo `search` do
/// índice, com casamento forte, as [`TOP`] mais fortes: o nome e o objetivo
/// dela como lacuna, e as regras, as decisões e os erros dela que casam como
/// fatos.
fn prior_spec_points(sources: &Sources<'_>) -> Vec<Proposed> {
    let terms = query_terms(sources.goal);
    let lines: Vec<&IndexLine> = sources.index.iter().filter(|line| line.name != sources.current).collect();
    let index = SearchIndex::build(lines.iter().enumerate().map(|(i, line)| (doc_id(i), line.search.as_str())));
    index
        .top(&terms, lines.len())
        .into_iter()
        .filter_map(|hit| lines.get(doc_pos(hit.id)).copied())
        .filter(|line| strong(&terms, &line.search))
        .take(TOP)
        .map(|line| {
            let goal = line.goal.as_deref().map(str::trim).filter(|g| !g.is_empty());
            let log = sources.prior.iter().find(|(name, _)| *name == line.name).map(|(_, log)| log);
            Proposed {
                block: "prior_specs".to_string(),
                gap: goal.map_or_else(|| line.name.clone(), |goal| format!("{}: {goal}", line.name)),
                from: FROM_PRIOR_SPEC,
                key: None,
                facts: prior_facts(&line.name, goal, log, &terms),
                reminders: Vec::new(),
            }
        })
        .collect()
}

/// Os fatos de uma spec anterior: as regras, as decisões e os erros dela que
/// casam com o objetivo, até [`PRIOR_FACTS`], cada um pelo título e pelo
/// comando que lê o item. Sem nenhum, o objetivo dela, pelo comando que lê a
/// especificação.
fn prior_facts(name: &str, goal: Option<&str>, log: Option<&SpecLog>, terms: &[String]) -> Vec<Fact> {
    let mut facts = Vec::new();
    if let Some(log) = log {
        let items: Vec<&SpecEvent> =
            log.visible().into_iter().filter(|e| PRIOR_FACT_TYPES.contains(&e.event_type.as_str())).collect();
        let codes = log.codes();
        let index = SearchIndex::build(items.iter().map(|e| (e.id, e.str_field("search").unwrap_or_default())));
        for hit in index.top(terms, PRIOR_FACTS) {
            let Some(item) = log.get(hit.id) else { continue };
            let (Some(text), Some(code)) = (title_of(item), codes.get(&item.id)) else { continue };
            facts.push(Fact { text, source: format!("mustard-rt run read agreed --spec {name} --term {code}") });
        }
    }
    if facts.is_empty()
        && let Some(goal) = goal
    {
        facts.push(Fact { text: goal.to_string(), source: format!("mustard-rt run read specification --spec {name}") });
    }
    facts
}

/// Põe cada lembrete no ponto da lista cujo texto (a lacuna e os fatos) casa
/// melhor com ele, sem passar de [`MAX_REMINDERS`] num ponto; sem casamento,
/// no primeiro ponto com lugar.
fn place(list: &mut [Proposed], found: Vec<Reminder>) {
    let docs: Vec<String> = list
        .iter()
        .map(|item| {
            let facts: Vec<&str> = item.facts.iter().map(|fact| fact.text.as_str()).collect();
            search_field(Some(&item.gap), &facts)
        })
        .collect();
    let index = SearchIndex::build(docs.iter().enumerate().map(|(i, doc)| (doc_id(i), doc.as_str())));
    let has_room = |list: &[Proposed], at: usize| list.get(at).is_some_and(|item| item.reminders.len() < MAX_REMINDERS);
    for reminder in found {
        let ranked = index.top(&query_terms(&reminder.text), list.len());
        let target = ranked
            .iter()
            .map(|hit| doc_pos(hit.id))
            .find(|at| has_room(list, *at))
            .or_else(|| (0..list.len()).find(|at| has_room(list, *at)));
        if let Some(item) = target.and_then(|at| list.get_mut(at)) {
            item.reminders.push(reminder);
        }
    }
}

/// Quantas raízes do pedido, sem repetir, o `search` tem.
fn shared_roots(terms: &[String], search: &str) -> usize {
    let roots: BTreeSet<&str> = search.split(' ').filter(|w| !w.is_empty()).collect();
    let terms: BTreeSet<&str> = terms.iter().map(String::as_str).collect();
    terms.into_iter().filter(|term| roots.contains(term)).count()
}

/// O casamento forte: pelo menos [`STRONG_ROOTS`] raízes do pedido em comum.
fn strong(terms: &[String], search: &str) -> bool {
    shared_roots(terms, search) >= STRONG_ROOTS
}

/// O texto como se compara: minúsculo, sem acento e com um espaço só entre
/// as palavras.
fn plain(text: &str) -> String {
    fold_accents(&text.to_lowercase()).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A posição de um documento como número, para a busca.
fn doc_id(pos: usize) -> u64 {
    u64::try_from(pos).unwrap_or(u64::MAX)
}

/// O número de um documento de volta à posição dele.
fn doc_pos(id: u64) -> usize {
    usize::try_from(id).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::{normalize, parse_log, render_line, stamp};
    use crate::domain::spec_index::goal_of;
    use serde_json::json;

    fn obj(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    /// Uma linha como o gravador deixa.
    fn ev(id: u64, event_type: &str, draft: Value) -> String {
        format!("{}\n", render_line(&stamp(normalize(obj(draft), event_type), id, None, "2026-09-14T09:00:00-03:00")))
    }

    /// Uma lição como o banco a guarda.
    fn lesson(id: u64, text: &str, keys: &[&str]) -> String {
        let draft = json!({
            "class": "defect",
            "text": text,
            "keys": keys,
            "applies_to": {"files": ["**"]},
            "found_in": {"spec": "antiga"},
        });
        format!(
            "{}\n",
            render_line(&stamp(lessons::normalize(obj(draft), None), id, None, "2026-09-14T09:00:00-03:00"))
        )
    }

    fn log_of(lines: &[String]) -> SpecLog {
        parse_log(&lines.concat())
    }

    const GOAL: &str = "Travar o merge enquanto houver pendência aberta.";

    fn sources<'a>(
        kinds: &'a [&'a str],
        bank: Option<&'a SpecLog>,
        index: &'a [IndexLine],
        prior: &'a [(String, SpecLog)],
    ) -> Sources<'a> {
        Sources {
            kinds,
            goal: GOAL,
            current: "atual",
            bank,
            lessons_file: ".claude/spec/lessons.ndjson",
            index,
            prior,
            map: None,
            condensed: false,
            lang: Locale::PtBr,
        }
    }

    fn reminders_in(list: &[Proposed]) -> Vec<&Reminder> {
        list.iter().flat_map(|item| item.reminders.iter()).collect()
    }

    /// Cada tipo traz as lacunas dele, na ordem dos blocos.
    #[test]
    fn each_kind_brings_its_gaps_in_block_order() {
        let feature = gaps(&["feature"]);
        assert_eq!(
            feature,
            [
                GapKey::WhoUses,
                GapKey::ExternalDeps,
                GapKey::Rules,
                GapKey::Limits,
                GapKey::Contracts,
                GapKey::Errors,
                GapKey::EdgeCases,
                GapKey::OutOfScope,
                GapKey::DoneProof,
            ]
        );
        assert_eq!(gaps(&["fix"]).len(), 5);
        assert_eq!(gaps(&["refactor"]).len(), 9);
        for kinds in [&["feature"][..], &["fix"], &["refactor"], &["feature", "fix", "refactor"]] {
            let ranks: Vec<usize> = gaps(kinds).iter().map(|gap| block_rank(gap.block())).collect();
            assert!(ranks.windows(2).all(|w| w[0] <= w[1]), "{kinds:?}: {ranks:?}");
        }
        assert!(gaps(&["outro"]).is_empty());
    }

    /// No pedido misto as listas se juntam, e a lacuna que as duas têm
    /// aparece uma vez só.
    #[test]
    fn a_mixed_request_joins_the_gap_lists_without_repeating_a_gap() {
        let mixed = gaps(&["feature", "fix"]);
        assert_eq!(mixed.len(), 9 + 5 - 1, "{mixed:?}");
        assert_eq!(mixed.iter().filter(|gap| **gap == GapKey::DoneProof).count(), 1);
        assert_eq!(gaps(&["fix", "feature"]), mixed, "the order of the kinds does not matter");
        let with_refactor = gaps(&["feature", "refactor"]);
        assert_eq!(with_refactor.len(), 9 + 9 - 2, "what stays out and the proof are shared: {with_refactor:?}");
    }

    #[test]
    fn the_kinds_are_read_in_one_order_and_an_unknown_one_is_named() {
        assert_eq!(parse_kinds("refactor, feature,feature"), Ok(vec!["feature", "refactor"]));
        assert_eq!(parse_kinds(" , "), Ok(Vec::new()));
        assert_eq!(parse_kinds("feature,bug"), Err("bug".to_string()));
    }

    /// Os pontos abertos vêm em ordem de bloco e, no bloco, de nascimento; o
    /// fechado sai, e o `closes` que aponta a primeira versão de um ponto
    /// revisto também o fecha.
    #[test]
    fn open_points_come_in_block_order_and_leave_when_closed() {
        let point = |id: u64, block: &str, gap: &str| {
            ev(id, "point", json!({"block": block, "gap": gap, "from": "gap", "status": "open", "origin": 1,
                "facts": [{"text": "f", "source": "mensagem 1"}]}))
        };
        let base = vec![
            ev(1, "message", json!({"author": "user", "text": "oi"})),
            point(2, "proof", "Como provar que ficou pronto"),
            point(3, "context", "Quem usa e para quê"),
            point(4, "rules", "Cada regra, com um exemplo com números"),
            point(5, "outro", "Um ponto de um bloco que não existe"),
        ];
        let ids = |log: &SpecLog| open_points(log).iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(&log_of(&base)), [3, 4, 2, 5]);

        let mut closed = base.clone();
        closed.push(ev(6, "point", json!({"block": "rules", "gap": "g", "from": "gap", "status": "closed",
            "closes": 4, "result": [1], "origin": 1})));
        assert_eq!(ids(&log_of(&closed)), [3, 2, 5]);

        let mut revised = base;
        revised.push(ev(6, "point", json!({"block": "context", "gap": "Quem usa", "from": "gap", "status": "open",
            "replaces": 3, "origin": 1, "facts": [{"text": "f2", "source": "mensagem 1"}]})));
        assert_eq!(ids(&log_of(&revised)), [6, 4, 2, 5], "the revision keeps the place of the point");
        revised.push(ev(7, "point", json!({"block": "context", "gap": "g", "from": "gap", "status": "not_applicable",
            "closes": 3, "reason": "não vale", "origin": 1})));
        assert_eq!(ids(&log_of(&revised)), [4, 2, 5], "closing the first version closes the revision");
    }

    /// Lado a lado: o levantamento e o índice leem o mesmo objetivo, também
    /// quando o primeiro foi revisto depois de outro `context` e quando o
    /// primeiro foi tirado.
    #[test]
    fn the_survey_and_the_index_read_the_same_goal() {
        let first = ev(2, "context", json!({"text": "Primeiro objetivo. Mais.", "origin": 1}));
        let message = ev(1, "message", json!({"author": "user", "text": "oi"}));
        let second = ev(3, "context", json!({"text": "Outro contexto.", "origin": 1}));
        let revised = ev(4, "context", json!({"text": "Objetivo revisto. Mais.", "replaces": 2, "origin": 1}));
        let removed = ev(4, "remove", json!({"targets": [2], "reason": "engano"}));
        for (lines, expected) in [
            (vec![message.clone(), first.clone(), second.clone()], Some("Primeiro objetivo.")),
            (vec![message.clone(), first.clone(), second.clone(), revised], Some("Objetivo revisto.")),
            (vec![message.clone(), first, second, removed], Some("Outro contexto.")),
            (vec![message], None),
        ] {
            let log = log_of(&lines);
            let surveyed = goal(&log).and_then(|e| e.str_field("text")).map(|t| crate::domain::spec_index::first_sentence(t).to_string());
            assert_eq!(surveyed.as_deref(), expected);
            assert_eq!(goal_of(&log), surveyed, "{lines:?}");
        }
    }

    /// As mensagens sem destino são as do usuário que nenhum registro aponta:
    /// a respondida continua sem destino, a do assistente e a tirada não
    /// entram.
    #[test]
    fn unrouted_messages_are_the_users_that_no_record_points_to() {
        let log = log_of(&[
            ev(1, "message", json!({"author": "user", "text": "decidida"})),
            ev(2, "message", json!({"author": "user", "text": "só respondida"})),
            ev(3, "message", json!({"author": "assistant", "text": "do assistente"})),
            ev(4, "message", json!({"author": "user", "text": "tirada"})),
            ev(5, "decision", json!({"text": "d", "keys": ["k"], "why": "w", "origin": 1})),
            ev(6, "response", json!({"author": "assistant", "text": "r", "reply_to": 2})),
            ev(7, "remove", json!({"targets": [4], "reason": "engano"})),
        ]);
        let ids: Vec<u64> = unrouted_messages(&log).iter().map(|m| m.id).collect();
        assert_eq!(ids, [2]);
    }

    /// Duas specs anteriores com cinco mensagens do usuário que casam com o
    /// pedido, duas delas já com decisão apontando, e respostas do
    /// assistente que casam: o levantamento traz três lembretes, nenhum das
    /// decididas nem do assistente, cada um dentro de um ponto da lista, sem
    /// ponto novo.
    #[test]
    fn at_most_three_reminders_none_already_decided_each_inside_a_point_and_never_an_assistant_reply() {
        let alfa = log_of(&[
            ev(1, "message", json!({"author": "user", "text": "O merge com pendência aberta passou sem aviso."})),
            ev(2, "message", json!({"author": "user", "text": "Travar o merge quando a pendência estiver aberta."})),
            ev(3, "decision", json!({"text": "Trava no merge.", "keys": ["merge"], "why": "w", "origin": 2})),
            ev(4, "response", json!({"author": "assistant", "text": GOAL, "reply_to": 1})),
            ev(5, "message", json!({"author": "assistant", "text": "Travar o merge com pendência aberta, sempre."})),
        ]);
        let beta = log_of(&[
            ev(1, "message", json!({"author": "user", "text": "A pendência aberta deve travar o merge."})),
            ev(2, "rule", json!({"text": "Pendência trava.", "keys": ["pendência"], "example": "e", "origin": 1})),
            ev(3, "message", json!({"author": "user", "text": "Merge travado por pendência aberta no dev."})),
            ev(4, "message", json!({"author": "user", "text": "Pendência aberta e merge: cobrar antes."})),
        ]);
        let prior = vec![("alfa".to_string(), alfa), ("beta".to_string(), beta)];
        let list = build(&sources(&["feature"], None, &[], &prior));
        assert_eq!(list.len(), gaps(&["feature"]).len(), "no point is born for a reminder");
        let found = reminders_in(&list);
        assert_eq!(found.len(), MAX_REMINDERS, "{found:?}");
        let picked: BTreeSet<(&str, u64)> = found.iter().map(|r| (r.spec.as_str(), r.message)).collect();
        assert_eq!(picked, BTreeSet::from([("alfa", 1), ("beta", 3), ("beta", 4)]));
        assert!(found.iter().all(|r| r.text != GOAL), "never an assistant reply");
    }

    /// Uma mensagem que divide só uma palavra com o objetivo não é lembrete;
    /// com duas, é.
    #[test]
    fn a_message_sharing_one_word_with_the_goal_is_not_a_reminder() {
        let old = log_of(&[
            ev(1, "message", json!({"author": "user", "text": "O merge ficou lento hoje."})),
            ev(2, "message", json!({"author": "user", "text": "O merge com pendência demorou."})),
        ]);
        let prior = [("antiga".to_string(), old)];
        let found = reminders(prior.iter().map(|(n, l)| (n.as_str(), l)), GOAL, MAX_REMINDERS);
        let ids: Vec<u64> = found.iter().map(|r| r.message).collect();
        assert_eq!(ids, [2], "{found:?}");
    }

    /// No empate, o nome da spec e depois o número da mensagem; a spec atual
    /// nunca dá lembrete a ela mesma.
    #[test]
    fn a_tie_goes_to_the_spec_name_and_the_current_spec_gives_no_reminder() {
        let same = |id: u64| log_of(&[ev(id, "message", json!({"author": "user", "text": "Merge com pendência aberta."}))]);
        let prior = vec![("b".to_string(), same(1)), ("a".to_string(), same(2)), ("atual".to_string(), same(1))];
        let list = build(&sources(&["fix"], None, &[], &prior));
        let found: Vec<(&str, u64)> = reminders_in(&list).iter().map(|r| (r.spec.as_str(), r.message)).collect();
        assert_eq!(found, [("a", 2), ("b", 1)]);
    }

    /// A lição que casa vira ponto, com o texto original como fato e a linha
    /// do banco como fonte, nunca o `search`.
    #[test]
    fn a_lesson_point_carries_the_lesson_text_and_its_line_never_the_search() {
        let bank = parse_log(
            &[
                lesson(1, "**Merge com pendência.** O merge não passa com pendência aberta.", &["merge", "pendência"]),
                lesson(2, "O cargo não está no PATH.", &["cargo"]),
            ]
            .concat(),
        );
        let list = build(&sources(&["fix"], Some(&bank), &[], &[]));
        let points: Vec<&Proposed> = list.iter().filter(|p| p.from == "lesson").collect();
        assert_eq!(points.len(), 1, "{list:?}");
        assert_eq!(points[0].gap, "Merge com pendência.");
        assert_eq!(points[0].block, "lessons");
        assert_eq!(points[0].facts[0].text, "**Merge com pendência.** O merge não passa com pendência aberta.");
        assert_eq!(points[0].facts[0].source, ".claude/spec/lessons.ndjson:1");
        let shown = points[0].to_value(Some(7)).to_string();
        assert!(!shown.contains("search") && !shown.contains("merg pendenc"), "{shown}");
    }

    /// A spec anterior que casa vira ponto, com as regras e as decisões que
    /// casam como fatos e o comando que as lê como fonte; a spec atual nunca é
    /// a própria spec anterior, e a que divide uma palavra só fica fora.
    #[test]
    fn a_prior_spec_point_lists_its_matching_rules_and_decisions() {
        let old = log_of(&[
            ev(1, "message", json!({"author": "user", "text": "x"})),
            ev(2, "context", json!({"text": "Travar o merge com pendência aberta.", "origin": 1})),
            ev(3, "rule", json!({"text": "**Merge travado.** Pendência aberta barra o merge.", "keys": ["merge"], "example": "e", "origin": 1})),
            ev(4, "decision", json!({"text": "A cobrança da pendência sai no merge.", "keys": ["pendência"], "why": "w", "origin": 1})),
            ev(5, "rule", json!({"text": "O título tem até 60 caracteres.", "keys": ["título"], "example": "e", "origin": 1})),
        ]);
        let line = |name: &str, goal: &str| IndexLine {
            name: name.to_string(),
            goal: Some(goal.to_string()),
            phase: Some("closed".to_string()),
            titles: Vec::new(),
            search: search_field(Some(goal), &[name]),
        };
        let index = vec![
            line("antiga", "Travar o merge com pendência aberta."),
            line("atual", GOAL),
            line("fraca", "O merge da página."),
        ];
        let prior = vec![("antiga".to_string(), old)];
        let list = build(&sources(&["fix"], None, &index, &prior));
        let points: Vec<&Proposed> = list.iter().filter(|p| p.from == "prior_spec").collect();
        assert_eq!(points.len(), 1, "{points:?}");
        assert_eq!(points[0].gap, "antiga: Travar o merge com pendência aberta.");
        let facts: Vec<(&str, &str)> = points[0].facts.iter().map(|f| (f.text.as_str(), f.source.as_str())).collect();
        assert!(facts.contains(&("Merge travado.", "mustard-rt run read agreed --spec antiga --term MSTD-RULE-0001")), "{facts:?}");
        assert!(
            facts.contains(&("A cobrança da pendência sai no merge.", "mustard-rt run read agreed --spec antiga --term MSTD-DEC-0001")),
            "{facts:?}"
        );
        assert!(!facts.iter().any(|(text, _)| text.contains("título")), "{facts:?}");
    }

    /// No condensado, todos os pontos vão para um bloco só.
    #[test]
    fn a_condensed_list_puts_every_point_in_one_block() {
        let mut given = sources(&["feature", "fix"], None, &[], &[]);
        given.condensed = true;
        let list = build(&given);
        assert_eq!(list.len(), 13);
        assert!(list.iter().all(|p| p.block == CONDENSED), "{list:?}");
    }

    /// A lacuna de quem depende, na refatoração, vem com o que o mapa já sabe
    /// do nome citado no objetivo: onde ele é declarado e quem importa o
    /// arquivo.
    #[test]
    fn a_gap_the_map_answers_comes_with_the_declaration_and_its_importers() {
        let map: ProjectMap = serde_json::from_value(json!({
            "modules": [
                {"path": "src/a.rs", "declarations": [{"kind": "function", "name": "record_birth", "line": 3}]},
                {"path": "src/b.rs", "deps": ["src/a.rs"]},
                {"path": "src/c.rs", "deps": ["src/a.rs"]},
            ]
        }))
        .unwrap();
        let mut given = sources(&["refactor"], None, &[], &[]);
        given.goal = "Tirar o `record_birth` do fluxo.";
        given.map = Some(&map);
        let list = build(&given);
        let dependents = list.iter().find(|p| p.key == Some(GapKey::Dependents)).unwrap();
        assert_eq!(
            dependents.facts,
            [
                Fact { text: "`record_birth` é declarado em src/a.rs, linha 3.".into(), source: "src/a.rs:3".into() },
                Fact {
                    text: "src/a.rs é importado por: src/b.rs, src/c.rs.".into(),
                    source: "mustard-rt run map importers --file src/a.rs".into(),
                },
            ]
        );
        assert!(list.iter().filter(|p| p.key != Some(GapKey::Dependents)).all(|p| p.facts.is_empty()));
    }

    /// Um ponto gravado registra o item da lista pela lacuna, com o rótulo em
    /// qualquer idioma ou pelo nome; o que falta continua na lista.
    #[test]
    fn a_recorded_gap_is_found_by_its_label_in_either_language_or_its_name() {
        let point = |id: u64, gap: &str| {
            ev(id, "point", json!({"block": "context", "gap": gap, "from": "gap", "status": "open", "origin": 1,
                "facts": [{"text": "f", "source": "mensagem 1"}]}))
        };
        let log = log_of(&[
            ev(1, "message", json!({"author": "user", "text": GOAL})),
            ev(2, "work_type", json!({"kinds": ["fix"], "origin": 1})),
            point(3, "o sintoma"),
            point(4, "How to reproduce it"),
            point(5, "expected_vs_actual"),
        ]);
        assert_eq!(missing_gaps(&log), [GapKey::Cause, GapKey::DoneProof]);
        let list = build(&sources(&["fix"], None, &[], &[]));
        let left: Vec<Option<GapKey>> = missing(&log, &list).iter().map(|p| p.key).collect();
        assert_eq!(left, [Some(GapKey::Cause), Some(GapKey::DoneProof)]);
        assert_eq!(list[0].recorded_by(&log).map(|p| p.id), Some(3));
    }

    /// Um ponto aberto, como o assistente grava, com a mensagem 2 de origem.
    fn open_point(id: u64, block: &str, gap: &str) -> String {
        ev(id, "point", json!({"block": block, "gap": gap, "from": "gap", "status": "open", "origin": 2,
            "facts": [{"text": "f", "source": "mensagem 2"}]}))
    }

    /// O fechamento do ponto `closes`.
    fn closing(id: u64, closes: u64) -> String {
        ev(id, "point", json!({"block": "b", "gap": "g", "from": "gap", "status": "closed", "closes": closes,
            "result": [2], "origin": 2}))
    }

    /// O levantamento só sai com o tipo de trabalho gravado, um ponto para
    /// cada lacuna dele e nenhum ponto aberto, e diz o que falta nessa ordem.
    #[test]
    fn leaving_the_survey_names_what_still_holds_it() {
        let mut lines = vec![
            ev(1, "state", json!({"phase": "survey", "author": "binary"})),
            ev(2, "message", json!({"author": "user", "text": GOAL})),
        ];
        assert_eq!(leave_survey(&log_of(&lines)), Err(SurveyGap::NotStarted));
        lines.push(ev(3, "work_type", json!({"kinds": ["fix"], "origin": 2})));
        assert_eq!(leave_survey(&log_of(&lines)), Err(SurveyGap::Unrecorded(gaps(&["fix"]))));
        for (id, key) in (4u64..).zip(gaps(&["fix"])) {
            lines.push(open_point(id, key.block(), key.name()));
        }
        let log = log_of(&lines);
        let Err(SurveyGap::Open(open)) = leave_survey(&log) else {
            panic!("the open points hold the survey");
        };
        assert_eq!(open.iter().map(|p| p.id).collect::<Vec<_>>(), [4, 5, 6, 7, 8]);
        for (id, closes) in (9u64..).zip(4u64..=8) {
            lines.push(closing(id, closes));
        }
        assert_eq!(leave_survey(&log_of(&lines)), Ok(()));
    }

    /// O passo depois de cada gravação segue os pontos abertos: fechar o
    /// último ponto de um bloco traz a revisão dele e o próximo ponto;
    /// fechar o último de todos traz a revisão com o revisor de fora e o fim.
    /// Depois da aprovação, nenhum passo.
    #[test]
    fn the_step_after_each_write_follows_the_open_points() {
        let mut lines = vec![
            ev(1, "state", json!({"phase": "survey", "author": "binary"})),
            ev(2, "message", json!({"author": "user", "text": GOAL})),
            open_point(3, "rules", "Cada regra"),
            open_point(4, "proof", "Como provar"),
        ];
        let before = log_of(&lines);
        lines.push(ev(5, "decision", json!({"text": "d", "keys": ["k"], "why": "w", "origin": 2})));
        let after = log_of(&lines);
        let steps = next_step(&before, &after);
        assert!(matches!(&steps[..], [SurveyStep::Point(p)] if p.id == 3), "{steps:?}");

        lines.push(closing(6, 3));
        let before = log_of(&lines[..lines.len() - 1]);
        let after = log_of(&lines);
        let steps = next_step(&before, &after);
        assert!(
            matches!(&steps[..], [SurveyStep::ReviewBlock { block, closed, records, outside_review: false }, SurveyStep::Point(p)]
                if block.as_str() == "rules" && closed == &[3] && records == &[2] && p.id == 4),
            "{steps:?}"
        );

        lines.push(closing(7, 4));
        let before = log_of(&lines[..lines.len() - 1]);
        let after = log_of(&lines);
        let steps = next_step(&before, &after);
        assert!(
            matches!(&steps[..], [SurveyStep::ReviewBlock { block, outside_review: true, .. }, SurveyStep::Done { unrouted }]
                if block.as_str() == "proof" && unrouted.is_empty()),
            "{steps:?}"
        );

        lines.push(ev(8, "state", json!({"phase": "approved", "author": "user"})));
        let before = log_of(&lines);
        lines.push(ev(9, "message", json!({"author": "user", "text": "Mais uma."})));
        assert!(next_step(&before, &log_of(&lines)).is_empty(), "no step after the approval");
    }
}
