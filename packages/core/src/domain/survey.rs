//! `survey` — o levantamento de uma spec: a lista de pontos que o `grill`
//! monta e a leitura dos pontos que continuam abertos.
//!
//! A lista ([`build`]) tem um ponto por lacuna do tipo de trabalho
//! ([`gaps`]): funcionalidade, defeito e refatoração, juntas sem repetir no
//! pedido misto; a lacuna que o mapa do projeto já responde vem com o fato e
//! a fonte. Dentro desses pontos, nunca num ponto novo, entram:
//! - as lições do banco que casam com o objetivo, pelo BM25 de
//!   `domain::search`, como fato, com o texto original, nunca o `search`; a
//!   regra do projeto fica fora, porque vale sempre;
//! - as specs anteriores que casam com o objetivo, pelo `search` do índice,
//!   como fatos, com as regras, as decisões e os erros delas que casam;
//!   quando a spec cita arquivos, entram as que mexeram em algum deles
//!   ([`prior_spec_facts`]);
//! - até 3 lembretes: mensagens do usuário nas specs anteriores que nenhum
//!   registro aponta ([`reminders`]).
//!
//! Quem grava os pontos é o assistente, pelo `write`, sobre esta lista, que
//! não é gravada: a mesma entrada dá sempre a mesma lista. Cada ponto gravado
//! é lido pelo par, o original e o fechamento, numa leitura só ([`points`]):
//! por ela, [`missing`] diz quais itens ainda não têm ponto gravado, e
//! [`open_points`] diz quais pontos continuam abertos, na ordem dos blocos.
//!
//! Todos os pontos vão para um bloco só ([`CONDENSED`]) e são mostrados de
//! uma vez, para um "sim". A spec antiga, com os pontos nos blocos de
//! [`BLOCKS`], continua lida na ordem deles ([`block_rank`]).
//!
//! Depois de cada gravação, [`next_step`] diz o passo seguinte: gravar os
//! pontos das lacunas que ainda não têm, o próximo ponto aberto e, sem ponto
//! aberto, o fim, com as mensagens do usuário sem destino. Nenhum bloco tem
//! revisão própria: a segunda leitura é a do revisor final, no fechamento.
//! [`leave_survey`] diz se a spec pode passar para o plano.
//!
//! Função pura: sem disco e sem relógio. Quem lê o banco, o índice, as specs
//! anteriores e o mapa é o comando.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::domain::citation::{cited_files, cited_names, file_citation};
use crate::domain::lessons;
use crate::domain::normalize::{Languages, Normalizer};
use crate::domain::project_map::{clean_path, importers};
use crate::domain::search::{SearchIndex, TOP, shared_words};
use crate::domain::spec_events::{Block, Refusal, SpecEvent, SpecLog, WORK_KINDS, search_field};
use crate::domain::spec_index::{IndexLine, title_of};
use crate::domain::spec_state::{State, birth_event, original_of};
use crate::domain::text::{fold_accents, one_line};
use crate::io::project_map::{MapReader, Need};
use crate::platform::i18n::{Locale, translate};

/// O bloco único do levantamento.
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
];

/// Quantos lembretes o levantamento traz no total.
pub const MAX_REMINDERS: usize = 3;

/// O casamento forte: quantas palavras do objetivo, no mínimo, uma mensagem
/// antiga ou uma spec anterior precisa ter para entrar. Com uma palavra só,
/// qualquer palavra comum ("pasta", "spec") traria lembrete e spec anterior.
/// A spec anterior só entra por palavras quando a spec do levantamento não
/// cita arquivo; citando, entra pelo arquivo em comum.
pub const STRONG_ROOTS: usize = 2;

/// Quantos fatos de uma spec anterior entram no levantamento.
pub const PRIOR_FACTS: usize = 3;

/// Quantos nomes citados no objetivo, e quantas declarações de cada um, o
/// mapa preenche.
const MAP_NAMES: usize = 3;

/// Quantos arquivos que importam o declarado entram no fato.
const MAP_IMPORTERS: usize = 20;

/// Os tipos da spec anterior que viram fato.
const PRIOR_FACT_TYPES: &[&str] = &["rule", "decision", "error"];

/// A origem de cada ponto da lista, no campo `from`.
const FROM_GAP: &str = "gap";

/// De onde vem cada fato da lista, no campo `kind`: o mapa do código, o banco
/// de lições, uma spec anterior ou uma citação do pedido do usuário.
pub const FACT_CODE: &str = "code";
pub const FACT_LESSON: &str = "lesson";
pub const FACT_PRIOR_SPEC: &str = "prior_spec";
pub const FACT_REQUEST: &str = "request";

/// Quem fecha um ponto do levantamento: o usuário, que decide o produto, ou o
/// assistente, que o responde do código.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    User,
    Assistant,
}

impl Owner {
    /// O nome estável do dono, como a resposta o mostra.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

/// O dono do ponto `point`, pela lacuna que o `gap` dele casa
/// ([`GapKey::matches`]); nada novo no registro. O ponto que não casa lacuna
/// nenhuma, como o de uma lição numa spec antiga, é do usuário.
#[must_use]
pub fn owner_of(point: &SpecEvent) -> Owner {
    let gap = point.str_field("gap").unwrap_or_default();
    GapKey::ALL.into_iter().find(|key| key.matches(gap)).map_or(Owner::User, GapKey::owner)
}

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
            Self::MeasuredReason | Self::MustNotChange | Self::RemovedAndUsers | Self::Moves | Self::Dependents | Self::GreenOrder | Self::BeforeAfter => {
                "refactor"
            }
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
        !written.is_empty() && (written == self.name() || [Locale::PtBr, Locale::EnUs].into_iter().any(|lang| plain(self.label(lang)) == written))
    }

    /// Quem fecha a lacuna, por uma tabela fixa. O usuário decide o produto:
    /// quem usa, as regras, os limites, os erros, os casos de borda, o que fica
    /// fora e o esperado contra o obtido. O assistente responde as outras do
    /// código, com a fonte.
    #[must_use]
    pub fn owner(self) -> Owner {
        match self {
            Self::WhoUses | Self::Rules | Self::Limits | Self::Errors | Self::EdgeCases | Self::OutOfScope | Self::ExpectedVsActual => Owner::User,
            Self::ExternalDeps
            | Self::Symptom
            | Self::Reproduction
            | Self::Cause
            | Self::MeasuredReason
            | Self::MustNotChange
            | Self::RemovedAndUsers
            | Self::Moves
            | Self::Dependents
            | Self::GreenOrder
            | Self::BeforeAfter
            | Self::Contracts
            | Self::DoneProof => Owner::Assistant,
        }
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
    let listed: Vec<&str> =
        event.fields.get("kinds").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::trim).collect()).unwrap_or_default();
    WORK_KINDS.iter().copied().filter(|kind| listed.contains(kind)).collect()
}

/// Os tipos de trabalho que o levantamento lembra quando ninguém os diz: os
/// do `work_type` gravado e, antes de haver um, o do começo da branch com que
/// a spec nasceu, que o usuário escolheu na abertura (`fix/x` dá `fix`).
/// Vazio quando nenhum dos dois diz um tipo.
#[must_use]
pub fn remembered_kinds(log: &SpecLog) -> Vec<&'static str> {
    if let Some(event) = work_type(log) {
        return kinds_of(event);
    }
    let branch = birth_event(log).and_then(|event| event.str_field("branch")).unwrap_or_default();
    let prefix = branch.trim().split_once('/').map(|(prefix, _)| prefix).unwrap_or_default();
    WORK_KINDS.iter().copied().filter(|kind| *kind == prefix).collect()
}

/// A lacuna `key` já tem um ponto gravado, aberto ou fechado, pela leitura
/// dos pares ([`points`]).
#[must_use]
pub fn gap_recorded(log: &SpecLog, key: GapKey) -> bool {
    points(log).iter().any(|point| point.from() == Some(FROM_GAP) && point.gap().is_some_and(|gap| key.matches(gap)))
}

/// As lacunas do tipo de trabalho gravado que ainda não têm ponto. Sem tipo
/// gravado, nenhuma.
#[must_use]
pub fn missing_gaps(log: &SpecLog) -> Vec<GapKey> {
    let kinds = work_type(log).map(kinds_of).unwrap_or_default();
    gaps(&kinds).into_iter().filter(|key| !gap_recorded(log, *key)).collect()
}

/// A ordem de um bloco na apresentação: o bloco único primeiro, depois os de
/// [`BLOCKS`], e o bloco desconhecido por último.
#[must_use]
pub fn block_rank(block: &str) -> usize {
    let block = block.trim();
    if block == CONDENSED {
        return 0;
    }
    BLOCKS.iter().position(|known| *known == block).map_or(BLOCKS.len() + 1, |i| i + 1)
}

/// Um ponto do levantamento, lido pelo par: o original, que nasceu aberto, e
/// o ponto que o fecha.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurveyPoint<'a> {
    first: u64,
    original: Option<&'a SpecEvent>,
    closing: Option<&'a SpecEvent>,
    shown: &'a SpecEvent,
}

impl<'a> SurveyPoint<'a> {
    /// O número da primeira versão do original: é por ele que o par se forma.
    #[must_use]
    pub fn first(&self) -> u64 {
        self.first
    }

    /// O original, na versão vigente, enquanto ele está na leitura.
    #[must_use]
    pub fn original(&self) -> Option<&'a SpecEvent> {
        self.original
    }

    /// O ponto que fecha este; sem ele, o ponto está aberto.
    #[must_use]
    pub fn closing(&self) -> Option<&'a SpecEvent> {
        self.closing
    }

    /// O ponto está aberto: o original existe e nada o fecha.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.closing.is_none()
    }

    /// O ponto como a leitura o mostra, com o número, o código, o bloco e a
    /// lacuna: o original, enquanto ele existe; depois que ele sai, o
    /// fechamento, que carrega a lacuna e a origem dele.
    #[must_use]
    pub fn shown(&self) -> &'a SpecEvent {
        self.shown
    }

    /// A lacuna do ponto (`gap`), lida pelo par.
    #[must_use]
    pub fn gap(&self) -> Option<&'a str> {
        self.shown.str_field("gap")
    }

    /// A origem do ponto (`from`), lida pelo par.
    #[must_use]
    pub fn from(&self) -> Option<&'a str> {
        self.shown.str_field("from").map(str::trim)
    }

    /// O bloco do ponto, lido pelo par.
    #[must_use]
    pub fn block(&self) -> &'a str {
        self.shown.str_field("block").unwrap_or_default().trim()
    }
}

/// A leitura única dos pontos do levantamento: cada ponto é o par formado
/// pelo original e pelo fechamento, casados pela primeira versão do original.
///
/// - Com fechamento, o ponto está fechado e cobre a lacuna do original,
///   exista ainda o original ou tenha ele saído por `remove` ou `purge`.
/// - Sem fechamento, o ponto está aberto enquanto o original existe; sem os
///   dois, o ponto não existe.
///
/// O original é o `point` visível com a situação `open`, na versão vigente; o
/// fechamento é o `point` visível que aponta em `closes` um ponto, por
/// qualquer versão dele. Em ordem da primeira versão do original.
#[must_use]
pub fn points(log: &SpecLog) -> Vec<SurveyPoint<'_>> {
    let mut pairs: BTreeMap<u64, (Option<&SpecEvent>, Option<&SpecEvent>)> = BTreeMap::new();
    for event in log.visible().into_iter().filter(|e| e.event_type == "point") {
        if let Some(first) = closed_first(log, event) {
            pairs.entry(first).or_default().1.get_or_insert(event);
        } else if event.str_field("status").map(str::trim) == Some("open") {
            pairs.entry(original_of(log, event)).or_default().0.get_or_insert(event);
        }
    }
    pairs
        .into_iter()
        .filter_map(|(first, (original, closing))| {
            let shown = original.or(closing)?;
            Some(SurveyPoint { first, original, closing, shown })
        })
        .collect()
}

/// A primeira versão do ponto que `event` fecha pelo `closes`; `None` quando
/// ele não fecha ponto nenhum.
pub(crate) fn closed_first(log: &SpecLog, event: &SpecEvent) -> Option<u64> {
    let target = log.get(event.int("closes")?).filter(|target| target.event_type == "point")?;
    Some(original_of(log, target))
}

/// Os pontos que continuam abertos, pela leitura dos pares ([`points`]). Em
/// ordem de bloco e, no mesmo bloco, pela ordem em que o ponto nasceu.
#[must_use]
pub fn open_points(log: &SpecLog) -> Vec<&SpecEvent> {
    let mut open: Vec<SurveyPoint<'_>> = points(log).into_iter().filter(SurveyPoint::is_open).collect();
    open.sort_by_key(|p| (block_rank(p.block()), p.first(), p.shown().id));
    open.into_iter().map(|p| p.shown()).collect()
}

/// Os pontos abertos que vão ao usuário, os dele ([`owner_of`]), na ordem de
/// [`open_points`]; os do assistente ele fecha do código, sem perguntar.
#[must_use]
pub fn ask_user(log: &SpecLog) -> Vec<&SpecEvent> {
    open_points(log).into_iter().filter(|point| owner_of(point) == Owner::User).collect()
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

/// Um passo do levantamento, que o `write` devolve depois de uma gravação.
#[derive(Debug, Clone, PartialEq)]
pub enum SurveyStep<'a> {
    /// Lacunas do tipo de trabalho ainda sem ponto, na ordem dos blocos:
    /// antes de qualquer ponto ser apresentado, os pontos delas são gravados.
    Record(Vec<GapKey>),
    /// O próximo ponto aberto: o mesmo, enquanto ele não fecha.
    Point(&'a SpecEvent),
    /// Não sobra ponto aberto: as mensagens do usuário que nenhum registro
    /// aponta.
    Done { unrouted: Vec<&'a SpecEvent> },
}

/// O passo do levantamento depois de uma gravação, lido do arquivo antes
/// (`before`) e depois (`after`) dela:
///
/// - nenhum fora do levantamento e do plano, e numa spec sem nenhum ponto e
///   sem lacuna por gravar (as specs antigas);
/// - enquanto alguma lacuna do tipo de trabalho não tem ponto (a gravação do
///   `grill` parou no meio, ou um ponto saiu), rodar o `grill` de novo, que
///   grava os que faltam; senão, o próximo ponto aberto, o primeiro na ordem
///   dos blocos; sem ponto aberto, o fim, com as mensagens do usuário sem
///   destino. O fim sai, com todas elas, na gravação que fecha o último ponto
///   e, depois dela, só na gravação que deixa uma mensagem de novo sem
///   destino (o registro que a apontava foi tirado), e com essa mensagem:
///   cada mensagem sem destino sai uma vez.
#[must_use]
pub fn next_step<'a>(before: &SpecLog, after: &'a SpecLog) -> Option<SurveyStep<'a>> {
    if !matches!(State::from_log(after).phase, Some("survey" | "plan")) {
        return None;
    }
    let unrecorded = missing_gaps(after);
    if !unrecorded.is_empty() {
        return Some(SurveyStep::Record(unrecorded));
    }
    if !after.visible().iter().any(|e| e.event_type == "point") {
        return None;
    }
    if let Some(point) = open_points(after).first().copied() {
        return Some(SurveyStep::Point(point));
    }
    let was = open_points(before);
    let mut unrouted = unrouted_messages(after);
    // Depois da gravação que fecha o último ponto, o fim só volta com a
    // mensagem que esta gravação deixou sem destino; a que já estava na
    // lista, ou saiu dela, não se mostra de novo.
    if was.is_empty() {
        let listed: BTreeSet<u64> = unrouted_messages(before).iter().map(|m| m.id).collect();
        unrouted.retain(|m| !listed.contains(&m.id));
    }
    (!was.is_empty() || !unrouted.is_empty()).then_some(SurveyStep::Done { unrouted })
}

/// A dica do fim do levantamento: com mensagens do usuário sem destino, a de
/// mostrá-las antes de seguir; sem nenhuma, só a de seguir para o plano.
#[must_use]
pub fn done_hint(unrouted: &[&SpecEvent], lang: Locale) -> &'static str {
    translate(if unrouted.is_empty() { "survey.done" } else { "survey.done_unrouted" }, lang)
}

/// O objetivo da spec: o primeiro `context` gravado, na versão vigente dele.
/// O objetivo revisto continua sendo o objetivo; o tirado deixa o lugar para
/// o `context` seguinte. É a mesma leitura do objetivo do índice.
#[must_use]
pub fn goal(log: &SpecLog) -> Option<&SpecEvent> {
    log.events.iter().filter(|e| e.event_type == "context" && e.int("replaces").is_none()).find_map(|e| log.current(e.id))
}

/// As mensagens do usuário, visíveis, que ficaram soltas: nenhum evento
/// visível as aponta em `origin`, e elas não chegaram com um ponto do
/// levantamento aberto. A mensagem que traz `during` foi dita no meio de um
/// ponto e já tem lugar nele, mesmo que o ponto tenha fechado sem citá-la.
/// Responder a uma mensagem não é destino para ela.
#[must_use]
pub fn unrouted_messages(log: &SpecLog) -> Vec<&SpecEvent> {
    let visible = log.visible();
    let routed: BTreeSet<u64> = visible.iter().filter_map(|e| e.int("origin")).collect();
    visible
        .into_iter()
        .filter(|e| e.event_type == "message" && e.str_field("author").map(str::trim) == Some("user"))
        .filter(|e| !routed.contains(&e.id) && e.int("during").is_none())
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
/// assistente nunca entram. As palavras são cortadas nas línguas
/// `languages`.
#[must_use]
pub fn reminders<'a>(prior: impl IntoIterator<Item = (&'a str, &'a SpecLog)>, goal: &str, max: usize, languages: &Languages) -> Vec<Reminder> {
    let mut normalizer = Normalizer::new(languages);
    let terms = normalizer.query(goal);
    if terms.is_empty() || max == 0 {
        return Vec::new();
    }
    let mut candidates: Vec<(&str, u64, &str, Vec<Vec<String>>)> = Vec::new();
    for (name, log) in prior {
        for message in unrouted_messages(log) {
            let Some(text) = message.str_field("text").map(str::trim).filter(|t| !t.is_empty()) else {
                continue;
            };
            let search = message.str_field("search").map_or_else(|| search_field(Some(text), &[]), str::to_string);
            candidates.push((name, message.id, text, normalizer.forms(&search)));
        }
    }
    let index = SearchIndex::build(candidates.iter().enumerate().map(|(i, c)| (doc_id(i), c.3.clone())));
    let mut found: Vec<Reminder> = index
        .top(&terms, candidates.len())
        .into_iter()
        .filter_map(|hit| {
            let (spec, message, text, search) = candidates.get(doc_pos(hit.id))?;
            strong(&terms, search).then(|| Reminder { spec: (*spec).to_string(), message: *message, text: (*text).to_string(), score: hit.score })
        })
        .collect();
    found.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.spec.cmp(&b.spec)).then(a.message.cmp(&b.message)));
    found.truncate(max);
    found
}

/// Um fato de um ponto da lista, com a fonte e de onde ele vem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub text: String,
    pub source: String,
    /// De onde o fato vem: [`FACT_CODE`], [`FACT_LESSON`], [`FACT_PRIOR_SPEC`]
    /// ou [`FACT_REQUEST`].
    pub kind: &'static str,
}

/// Um item da lista que o `grill` monta: o ponto de uma lacuna, que ele
/// grava aberto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposed {
    pub block: String,
    pub gap: String,
    /// A lacuna do ponto.
    pub key: GapKey,
    /// Os fatos que a lista já traz, cada um com a fonte.
    pub facts: Vec<Fact>,
    pub reminders: Vec<Reminder>,
}

impl Proposed {
    /// O ponto como o `grill` o grava, com a mensagem de origem.
    #[must_use]
    pub fn to_value(&self, origin: Option<u64>) -> Value {
        let mut out = Map::new();
        out.insert("block".into(), Value::from(self.block.as_str()));
        out.insert("gap".into(), Value::from(self.gap.as_str()));
        out.insert("from".into(), Value::from(FROM_GAP));
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
                    item.insert("kind".into(), Value::from(fact.kind));
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

    /// O ponto que registra este item pela leitura dos pares ([`points`]),
    /// com a situação dele, `open` ou `closed`: o ponto de lacuna com a mesma
    /// lacuna, um aberto antes de um fechado. A lacuna é reconhecida pelo
    /// rótulo em qualquer idioma ou pelo nome. O ponto vem pelo original e,
    /// se ele saiu, pelo fechamento.
    #[must_use]
    pub fn standing<'a>(&self, log: &'a SpecLog) -> Option<(&'a SpecEvent, &'static str)> {
        let points = points(log);
        let is = |p: &SurveyPoint<'_>| p.from() == Some(FROM_GAP) && p.gap().is_some_and(|gap| self.key.matches(gap));
        let found = |open: bool| points.iter().find(|p| p.is_open() == open && is(p));
        found(true).map(|p| (p.shown(), "open")).or_else(|| found(false).map(|p| (p.shown(), "closed")))
    }
}

/// Os itens da lista que ainda não têm ponto gravado, pela leitura dos pares,
/// na ordem da lista.
#[must_use]
pub fn missing<'a>(log: &SpecLog, list: &'a [Proposed]) -> Vec<&'a Proposed> {
    list.iter().filter(|item| item.standing(log).is_none()).collect()
}

/// O que a lista usa, já lido por quem chama.
pub struct Sources<'a> {
    /// Os tipos de trabalho.
    pub kinds: &'a [&'a str],
    /// O texto do objetivo.
    pub goal: &'a str,
    /// A parte do agente do objetivo, vazia quando não há: é nela que o
    /// objetivo gravado pela forma de três partes cita os nomes de código.
    pub goal_agent: &'a str,
    /// A spec do levantamento, que nunca é a própria spec anterior.
    pub current: &'a str,
    /// O banco de lições, quando existe.
    pub bank: Option<&'a SpecLog>,
    /// O caminho do banco de lições a partir da raiz do projeto, para a fonte
    /// do fato de cada lição.
    pub lessons_file: &'a str,
    /// As linhas de spec do índice.
    pub index: &'a [IndexLine],
    /// O arquivo de eventos de cada spec do projeto, a do levantamento
    /// inclusive: dela vêm os arquivos que a spec cita, além do objetivo.
    pub prior: &'a [(String, SpecLog)],
    /// Como o mapa do projeto se lê, quando há um: cada pergunta lê só as
    /// tabelas dela, e só a lacuna de quem depende pergunta.
    pub map: Option<&'a MapReader<'a>>,
    /// O idioma dos rótulos.
    pub lang: Locale,
    /// As línguas em que as palavras das buscas são cortadas.
    pub languages: &'a Languages,
}

/// A lista de pontos do levantamento, toda no bloco único: um ponto por
/// lacuna do tipo e, dentro desses pontos, as lições que casam, as specs
/// anteriores que casam e os lembretes. Cada lição e cada spec anterior vira
/// fato do ponto cuja lacuna casa melhor com ela, e cada lembrete vai para o
/// ponto cujo texto casa melhor com ele; sem casamento, para o primeiro.
/// Nenhum ponto nasce para uma lição, uma spec anterior ou um lembrete.
#[must_use]
pub fn build(sources: &Sources<'_>) -> Vec<Proposed> {
    let mut list: Vec<Proposed> = gaps(sources.kinds)
        .into_iter()
        .map(|key| Proposed {
            block: CONDENSED.to_string(),
            gap: key.label(sources.lang).to_string(),
            key,
            facts: map_facts(key, sources),
            reminders: Vec::new(),
        })
        .collect();
    let mut by_gap = PointIndex::new(&list, sources.languages);
    for (matched, facts) in lesson_facts(sources).into_iter().chain(prior_spec_facts(sources)) {
        let at = by_gap.ranked(&matched).into_iter().next().unwrap_or_default();
        if let Some(item) = list.get_mut(at) {
            item.facts.extend(facts);
        }
    }
    let prior = sources.prior.iter().filter(|(name, _)| name != sources.current).map(|(name, log)| (name.as_str(), log));
    let mut by_text = PointIndex::new(&list, sources.languages);
    for reminder in reminders(prior, sources.goal, MAX_REMINDERS, sources.languages) {
        let has_room = |at: &usize| list.get(*at).is_some_and(|item| item.reminders.len() < MAX_REMINDERS);
        let target = by_text.ranked(&reminder.text).into_iter().find(has_room).or_else(|| (0..list.len()).find(has_room));
        if let Some(item) = target.and_then(|at| list.get_mut(at)) {
            item.reminders.push(reminder);
        }
    }
    list
}

/// Structural evidence for the open question. Declarations and importers
/// guide contract/flow investigation; associated tests are candidates, never
/// proof of coverage. Product choices still need the user's answer.
fn map_facts(key: GapKey, sources: &Sources<'_>) -> Vec<Fact> {
    let consumers = matches!(key, GapKey::Dependents | GapKey::WhoUses | GapKey::RemovedAndUsers | GapKey::Moves);
    let validation = matches!(key, GapKey::DoneProof | GapKey::GreenOrder | GapKey::Reproduction);
    let contracts = key == GapKey::Contracts;
    let Some(read) = sources.map.filter(|_| consumers || validation || contracts) else {
        return Vec::new();
    };
    let mut facts: Vec<Fact> = Vec::new();
    let mut push = |fact: Fact| {
        if !facts.contains(&fact) {
            facts.push(fact);
        }
    };
    let mut names: Vec<String> = Vec::new();
    for name in cited_names(sources.goal_agent).into_iter().chain(cited_names(sources.goal)) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    for name in names.into_iter().take(MAP_NAMES) {
        let declared = read(Need::Declarations { file: None, name: &name }).map(|map| map.declared(&name));
        for (path, line) in declared.unwrap_or_default().into_iter().take(MAP_NAMES) {
            push(Fact {
                text: translate("survey.fact_declared", sources.lang).replace("{name}", &name).replace("{path}", &path).replace("{line}", &line.to_string()),
                source: format!("{path}:{line}"),
                kind: FACT_CODE,
            });
            if validation
                && let Some(tests) = read(Need::Tests(&path)).ok().and_then(|map| crate::domain::project_map::tests_for(&map, &path).ok())
                && (tests.inline || !tests.files.is_empty())
            {
                push(Fact {
                    text: translate("survey.fact_test_candidates", sources.lang)
                        .replace("{path}", &path)
                        .replace("{tests}", &tests.files.join(", "))
                        .replace("{inline}", if tests.inline { "true" } else { "false" }),
                    source: format!("mustard-rt run map tests --file {path}"),
                    kind: FACT_CODE,
                });
            }
            if !consumers {
                continue;
            }
            let users = read(Need::Importers(&path)).and_then(|map| importers(&map, &path)).unwrap_or_default();
            if users.is_empty() {
                continue;
            }
            let mut listed = users.iter().take(MAP_IMPORTERS).cloned().collect::<Vec<_>>().join(", ");
            if users.len() > MAP_IMPORTERS {
                listed.push_str(", …");
            }
            push(Fact {
                text: translate("survey.fact_importers", sources.lang).replace("{path}", &path).replace("{importers}", &listed),
                source: format!("mustard-rt run map importers --file {path}"),
                kind: FACT_CODE,
            });
        }
    }
    facts
}

/// Um fato por lição do banco que casa com o objetivo, as mais fortes
/// primeiro: o texto original dela, com o arquivo e a linha do banco como
/// fonte, junto do texto que procura o ponto dela. A regra do projeto nunca
/// entra: ela vale sempre, e repeti-la em cada spec não acrescenta nada. Ela
/// sai antes da busca, para que centenas de regras não tomem o lugar dos
/// defeitos, das armadilhas e das preferências entre as mais fortes. As
/// lições são as que a leitura do banco mostra ([`lessons::kept`]): a linha
/// que retira lições não é lição.
///
/// A lição que diz os arquivos onde vale também sai antes da busca
/// ([`lessons::reaches_waves_by_files`]): o pedido de cada onda que mexe
/// neles já a leva. Ficam as lições que só dizem o subprojeto ou a skill.
fn lesson_facts(sources: &Sources<'_>) -> Vec<(String, Vec<Fact>)> {
    let Some(bank) = sources.bank else {
        return Vec::new();
    };
    let asked: Vec<&SpecEvent> = lessons::kept(bank)
        .into_iter()
        .filter(|lesson| lesson.event_type != lessons::PROJECT_RULE)
        .filter(|lesson| !lessons::reaches_waves_by_files(lesson))
        .collect();
    lessons::matching_among(&asked, sources.goal, sources.languages)
        .into_iter()
        .filter_map(|hit| {
            let lesson = bank.get(hit.id)?;
            let text = lesson.str_field("text").map(str::trim).filter(|t| !t.is_empty())?;
            let fact = Fact { text: text.to_string(), source: format!("{}:{}", sources.lessons_file, lesson.line), kind: FACT_LESSON };
            Some((text.to_string(), vec![fact]))
        })
        .collect()
}

/// Os fatos de cada spec anterior que entra no levantamento, as [`TOP`] mais
/// fortes: as regras, as decisões e os erros dela que casam, ou o objetivo
/// dela ([`prior_facts`]), junto do texto que procura o ponto deles, o
/// objetivo e os fatos.
///
/// Quem entra: se a spec do levantamento cita arquivos, as specs que
/// mexeram em algum deles ([`by_files`]); sem arquivo citado, as que casam
/// com o objetivo pelo `search` do índice ([`by_words`]).
fn prior_spec_facts(sources: &Sources<'_>) -> Vec<(String, Vec<Fact>)> {
    let mut normalizer = Normalizer::new(sources.languages);
    let terms = normalizer.query(sources.goal);
    let cited = files_cited_by_the_survey(sources);
    let entering = if cited.is_empty() { by_words(sources, &terms, &mut normalizer) } else { by_files(sources, &cited) };
    entering
        .into_iter()
        .map(|line| {
            let goal = line.goal.as_deref().map(str::trim).filter(|g| !g.is_empty());
            let log = sources.prior.iter().find(|(name, _)| *name == line.name).map(|(_, log)| log);
            let facts = prior_facts(&line.name, goal, log, &terms, &mut normalizer);
            let texts: Vec<&str> = facts.iter().map(|fact| fact.text.as_str()).collect();
            (search_field(goal, &texts), facts)
        })
        .collect()
}

/// As specs anteriores que casam com o objetivo pelo `search` do índice, com
/// casamento forte, as mais fortes primeiro.
fn by_words<'a>(sources: &Sources<'a>, terms: &[Vec<String>], normalizer: &mut Normalizer) -> Vec<&'a IndexLine> {
    let lines: Vec<(&IndexLine, Vec<Vec<String>>)> =
        sources.index.iter().filter(|line| line.name != sources.current).map(|line| (line, normalizer.forms(&line.search))).collect();
    let index = SearchIndex::build(lines.iter().enumerate().map(|(i, (_, words))| (doc_id(i), words.clone())));
    index
        .top(terms, lines.len())
        .into_iter()
        .filter_map(|hit| lines.get(doc_pos(hit.id)))
        .filter(|(_, words)| strong(terms, words))
        .map(|(line, _)| *line)
        .take(TOP)
        .collect()
}

/// As specs anteriores que mexeram em algum dos arquivos `cited`, as que
/// mexeram em mais arquivos primeiro e, no empate, pelo nome. A spec sem
/// arquivo de eventos lido não entra: não se sabe em que ela mexeu.
fn by_files<'a>(sources: &Sources<'a>, cited: &BTreeSet<String>) -> Vec<&'a IndexLine> {
    let mut shared: Vec<(usize, &IndexLine)> = sources
        .index
        .iter()
        .filter(|line| line.name != sources.current)
        .filter_map(|line| {
            let (_, log) = sources.prior.iter().find(|(name, _)| *name == line.name)?;
            let common = files_touched_by(log).intersection(cited).count();
            (common > 0).then_some((common, line))
        })
        .collect();
    shared.sort_by(|(more, one), (fewer, other)| fewer.cmp(more).then_with(|| one.name.cmp(&other.name)));
    shared.into_iter().map(|(_, line)| line).take(TOP).collect()
}

/// Os arquivos que a spec do levantamento cita: na parte do agente do
/// objetivo (o texto do usuário não leva caminho) e, nos itens já gravados,
/// na parte do agente e na fonte dos fatos. O ponto que o próprio `grill`
/// gravou (autor `binary`) fica de fora: a lista não se alimenta do que ela
/// mesma pôs na spec, e chamar o levantamento de novo dá a mesma lista.
fn files_cited_by_the_survey(sources: &Sources<'_>) -> BTreeSet<String> {
    let mut files: BTreeSet<String> = cited_files(sources.goal_agent).into_iter().collect();
    if let Some((_, log)) = sources.prior.iter().find(|(name, _)| name == sources.current) {
        let recorded = log.visible().into_iter().filter(|event| event.str_field("author") != Some("binary"));
        files.extend(recorded.flat_map(|event| files_of_item(event, false)));
    }
    files
}

/// Os arquivos em que uma spec anterior mexeu: os das entregas (`files`) e
/// os que os itens combinados e a especificação dela citam. A conversa e as
/// ondas, com os pedidos que levam as regras do projeto inteiro, não contam.
fn files_touched_by(log: &SpecLog) -> BTreeSet<String> {
    let mut files = BTreeSet::new();
    for event in log.visible() {
        if event.event_type == "delivered" {
            let delivered = event.fields.get("files").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str);
            files.extend(delivered.map(clean_path));
        } else if matches!(event.block(), Some(Block::Agreed | Block::Specification)) {
            files.extend(files_of_item(event, true));
        }
    }
    files
}

/// Os arquivos que um item cita: na parte do agente, no texto quando
/// `with_text` (o item antigo não tem a parte do agente, e o caminho ficava no
/// texto) e na fonte de cada fato.
fn files_of_item(event: &SpecEvent, with_text: bool) -> Vec<String> {
    let mut texts = vec![event.str_field("agent")];
    if with_text {
        texts.push(event.str_field("text"));
    }
    let facts = event.fields.get("facts").and_then(Value::as_array).into_iter().flatten();
    let sources = facts.filter_map(|fact| fact.get("source").and_then(Value::as_str)).filter_map(file_citation).map(|(path, _)| clean_path(&path));
    texts.into_iter().flatten().flat_map(cited_files).chain(sources).collect()
}

/// Os fatos de uma spec anterior: as regras, as decisões e os erros dela que
/// casam com o objetivo, até [`PRIOR_FACTS`], cada um pelo título e pelo
/// comando que lê o item. Sem nenhum, o objetivo dela, pelo comando que lê a
/// especificação.
fn prior_facts(name: &str, goal: Option<&str>, log: Option<&SpecLog>, terms: &[Vec<String>], normalizer: &mut Normalizer) -> Vec<Fact> {
    let mut facts = Vec::new();
    if let Some(log) = log {
        let items: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| PRIOR_FACT_TYPES.contains(&e.event_type.as_str())).collect();
        let codes = log.codes();
        let index = SearchIndex::build(items.iter().map(|e| (e.id, normalizer.forms(e.str_field("search").unwrap_or_default()))).collect::<Vec<_>>());
        for hit in index.top(terms, PRIOR_FACTS) {
            let Some(item) = log.get(hit.id) else {
                continue;
            };
            let (Some(text), Some(code)) = (title_of(item), codes.get(&item.id)) else {
                continue;
            };
            facts.push(Fact { text, source: format!("mustard-rt run read agreed --spec {name} --term {code}"), kind: FACT_PRIOR_SPEC });
        }
    }
    if facts.is_empty()
        && let Some(goal) = goal
    {
        facts.push(Fact { text: goal.to_string(), source: format!("mustard-rt run read specification --spec {name}"), kind: FACT_PRIOR_SPEC });
    }
    facts
}

/// A busca pelo BM25 sobre os pontos da lista, pelo texto de cada um (a
/// lacuna e os fatos), com as palavras cortadas nas línguas do projeto.
struct PointIndex {
    normalizer: Normalizer,
    index: SearchIndex,
    len: usize,
}

impl PointIndex {
    fn new(list: &[Proposed], languages: &Languages) -> Self {
        let mut normalizer = Normalizer::new(languages);
        let docs: Vec<Vec<Vec<String>>> = list
            .iter()
            .map(|item| {
                let facts: Vec<&str> = item.facts.iter().map(|fact| fact.text.as_str()).collect();
                normalizer.forms(&search_field(Some(&item.gap), &facts))
            })
            .collect();
        let index = SearchIndex::build(docs.into_iter().enumerate().map(|(i, doc)| (doc_id(i), doc)));
        Self { normalizer, index, len: list.len() }
    }

    /// As posições dos pontos que casam com `text`, do mais parecido ao
    /// menos; vazio quando nenhum casa.
    fn ranked(&mut self, text: &str) -> Vec<usize> {
        self.index.top(&self.normalizer.query(text), self.len).into_iter().map(|hit| doc_pos(hit.id)).collect()
    }
}

/// O casamento forte: pelo menos [`STRONG_ROOTS`] palavras do pedido com
/// alguma forma no documento.
fn strong(terms: &[Vec<String>], doc: &[Vec<String>]) -> bool {
    shared_words(terms, doc) >= STRONG_ROOTS
}

/// O texto como se compara: minúsculo, sem acento e com um espaço só entre
/// as palavras.
fn plain(text: &str) -> String {
    one_line(&fold_accents(&text.to_lowercase()))
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
    use crate::domain::project_map::ProjectMap;
    use crate::domain::spec_events::{normalize, parse_log, render_line, stamp};
    use crate::domain::spec_index::goal_of;
    use crate::io::project_map as store;
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

    /// Uma lição como o banco a guarda, valendo para uma skill: o levantamento
    /// pergunta por ela.
    fn lesson(id: u64, text: &str, keys: &[&str]) -> String {
        lesson_for(id, text, keys, json!({"skill": "backend"}))
    }

    /// Uma lição como o banco a guarda, com o `applies_to` dado.
    fn lesson_for(id: u64, text: &str, keys: &[&str], applies_to: Value) -> String {
        let draft = json!({
            "class": "defect",
            "text": text,
            "keys": keys,
            "applies_to": applies_to,
            "found_in": {"spec": "antiga"},
        });
        format!("{}\n", render_line(&stamp(lessons::normalize(obj(draft), None), id, None, "2026-09-14T09:00:00-03:00")))
    }

    fn log_of(lines: &[String]) -> SpecLog {
        parse_log(&lines.concat())
    }

    const GOAL: &str = "Travar o merge enquanto houver pendência aberta.";

    fn sources<'a>(kinds: &'a [&'a str], bank: Option<&'a SpecLog>, index: &'a [IndexLine], prior: &'a [(String, SpecLog)]) -> Sources<'a> {
        Sources {
            kinds,
            goal: GOAL,
            goal_agent: "",
            current: "atual",
            bank,
            lessons_file: ".claude/spec/lessons.ndjson",
            index,
            prior,
            map: None,
            lang: Locale::PtBr,
            languages: &LANGUAGES,
        }
    }

    /// As línguas de um projeto que escreve em português e programa em inglês.
    static LANGUAGES: std::sync::LazyLock<Languages> = std::sync::LazyLock::new(|| Languages::new(["pt-BR", "en-US"]));

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
            ev(
                id,
                "point",
                json!({"block": block, "gap": gap, "from": "gap", "status": "open", "origin": 1,
                "facts": [{"text": "f", "source": "mensagem 1"}]}),
            )
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
        closed.push(ev(
            6,
            "point",
            json!({"block": "rules", "gap": "g", "from": "gap", "status": "closed",
            "closes": 4, "result": [1], "origin": 1}),
        ));
        assert_eq!(ids(&log_of(&closed)), [3, 2, 5]);

        let mut revised = base;
        revised.push(ev(
            6,
            "point",
            json!({"block": "context", "gap": "Quem usa", "from": "gap", "status": "open",
            "replaces": 3, "origin": 1, "facts": [{"text": "f2", "source": "mensagem 1"}]}),
        ));
        assert_eq!(ids(&log_of(&revised)), [6, 4, 2, 5], "the revision keeps the place of the point");
        revised.push(ev(
            7,
            "point",
            json!({"block": "context", "gap": "g", "from": "gap", "status": "not_applicable",
            "closes": 3, "reason": "não vale", "origin": 1}),
        ));
        assert_eq!(ids(&log_of(&revised)), [4, 2, 5], "closing the first version closes the revision");
    }

    /// O objetivo do levantamento é o primeiro `context` vigente, e o índice
    /// mostra a primeira frase dele: na ordem, depois de revisto (mesmo com
    /// outro `context` no meio) e depois de tirado.
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

    /// A mensagem dita no meio de um ponto (`during`) já tem lugar nele e não
    /// fica solta, tenha o ponto fechado citando-a ou não; a que chegou sem
    /// ponto aberto e sem destino continua solta, e o ponto sem mensagem
    /// nenhuma continua como está.
    #[test]
    fn a_message_said_during_a_point_is_not_unrouted() {
        let log = log_of(&[
            ev(1, "message", json!({"author": "user", "text": GOAL})),
            ev(
                2,
                "point",
                json!({"block": "proof", "gap": "Como provar?", "from": "gap", "status": "open", "origin": 1,
                "facts": [{"text": "f", "source": "mensagem 1"}]}),
            ),
            ev(3, "message", json!({"author": "user", "text": "8 é fixo?", "during": 2})),
            ev(4, "message", json!({"author": "user", "text": "e o painel?"})),
            ev(5, "message", json!({"author": "user", "text": "não entendi", "during": 2})),
        ]);
        let ids: Vec<u64> = unrouted_messages(&log).iter().map(|m| m.id).collect();
        assert_eq!(ids, [4]);
    }

    /// Duas specs anteriores com cinco mensagens do usuário que casam com o
    /// pedido, duas delas já com decisão apontando, e respostas do
    /// assistente que casam: o levantamento traz três lembretes, nenhum das
    /// decididas nem do assistente, cada um dentro de um ponto da lista, sem
    /// ponto novo.
    #[test]
    fn at_most_three_reminders_none_already_decided_each_inside_a_point_and_never_an_assistant_reply() {
        let alpha = log_of(&[
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
        let prior = vec![("alpha".to_string(), alpha), ("beta".to_string(), beta)];
        let list = build(&sources(&["feature"], None, &[], &prior));
        assert_eq!(list.len(), gaps(&["feature"]).len(), "no point is born for a reminder");
        let found = reminders_in(&list);
        assert_eq!(found.len(), MAX_REMINDERS, "{found:?}");
        let picked: BTreeSet<(&str, u64)> = found.iter().map(|r| (r.spec.as_str(), r.message)).collect();
        assert_eq!(picked, BTreeSet::from([("alpha", 1), ("beta", 3), ("beta", 4)]));
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
        let found = reminders(prior.iter().map(|(n, l)| (n.as_str(), l)), GOAL, MAX_REMINDERS, &LANGUAGES);
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

    /// Os fatos de um tipo, em todos os pontos da lista.
    fn facts_of<'a>(list: &'a [Proposed], kind: &str) -> Vec<&'a Fact> {
        list.iter().flat_map(|p| p.facts.iter()).filter(|f| f.kind == kind).collect()
    }

    /// A lição que casa vira fato do ponto de lacuna mais parecido com ela,
    /// com o texto original e a linha do banco como fonte, nunca o `search`.
    /// Nenhum ponto nasce para ela: a funcionalidade tem os nove de sempre.
    #[test]
    fn a_lesson_becomes_a_fact_of_the_closest_point_never_a_point() {
        let edge = "**Borda do merge.** Os casos de borda do merge com pendência aberta.";
        let bank = parse_log(&[lesson(1, edge, &["merge", "pendência"]), lesson(2, "O cargo não está no PATH.", &["cargo"])].concat());
        let list = build(&sources(&["feature"], Some(&bank), &[], &[]));
        assert_eq!(list.len(), 9, "{list:?}");
        let facts = facts_of(&list, FACT_LESSON);
        assert_eq!(facts.len(), 1, "{list:?}");
        assert_eq!(facts[0].text, edge);
        assert_eq!(facts[0].source, ".claude/spec/lessons.ndjson:1");
        let holder = list.iter().find(|p| p.facts.iter().any(|f| f.kind == FACT_LESSON)).expect("the point of the lesson");
        assert_eq!(holder.key, GapKey::EdgeCases, "{list:?}");
        let shown = holder.to_value(Some(7)).to_string();
        assert!(!shown.contains("search") && !shown.contains("merg pendenc") && shown.contains("\"kind\":\"lesson\""), "{shown}");
    }

    /// A lição que diz os arquivos onde vale, o projeto todo incluído, chega
    /// ao pedido de toda onda que mexe neles e fica fora do levantamento; a
    /// que só diz o subprojeto ou a skill continua entrando como fato, com o
    /// mesmo casamento com o objetivo.
    #[test]
    fn a_lesson_that_names_files_never_enters_the_survey() {
        let text = |name: &str| format!("**{name}.** O merge não passa com pendência aberta.");
        let bank = log_of(&[
            lesson_for(1, &text("Por arquivo"), &["merge", "pendência"], json!({"files": ["apps/rt/src/**"]})),
            lesson_for(2, &text("Projeto todo"), &["merge", "pendência"], json!({"files": ["**"]})),
            lesson_for(3, &text("Por skill"), &["merge", "pendência"], json!({"skill": "backend"})),
            lesson_for(4, &text("Por subprojeto"), &["merge", "pendência"], json!({"subproject": "apps/rt"})),
            lesson_for(5, &text("Arquivo e skill"), &["merge", "pendência"], json!({"files": ["src/main.rs"], "skill": "backend"})),
        ]);
        let list = build(&sources(&["fix"], Some(&bank), &[], &[]));
        let mut asked: Vec<&str> = facts_of(&list, FACT_LESSON).iter().map(|f| f.text.as_str()).collect();
        asked.sort_unstable();
        assert_eq!(asked, [text("Por skill"), text("Por subprojeto")], "{list:?}");
    }

    /// O levantamento lê as lições como a leitura do banco as mostra: a
    /// linha que retira uma lição não vira ponto, nem quando traz um texto
    /// que casa com o objetivo, como a que o gravador aceitava antes de
    /// recusar campo a mais na retirada. A lição retirada também não.
    #[test]
    fn a_retirement_line_never_becomes_a_lesson_point() {
        let retirement = json!({"targets": [2], "reason": "saiu", "text": "O merge com pendência aberta saiu."});
        let bank = log_of(&[
            lesson(1, "**Merge com pendência.** O merge não passa com pendência aberta.", &["merge", "pendência"]),
            lesson(2, "**Pendência no merge.** A pendência aberta segura o merge.", &["merge", "pendência"]),
            format!("{}\n", render_line(&stamp(lessons::normalize(obj(retirement), None), 3, None, "2026-09-14T09:00:00-03:00"))),
        ]);
        let list = build(&sources(&["fix"], Some(&bank), &[], &[]));
        let lines: Vec<&str> = facts_of(&list, FACT_LESSON).iter().map(|f| f.source.as_str()).collect();
        let shown: Vec<String> = lessons::kept(&bank).iter().map(|l| format!(".claude/spec/lessons.ndjson:{}", l.line)).collect();
        assert_eq!(lines, shown, "{list:?}");
        assert_eq!(lines, [".claude/spec/lessons.ndjson:1"]);
    }

    /// A spec anterior que casa entra com as regras e as decisões que casam
    /// como fatos e o comando que as lê como fonte, sem ponto próprio; a spec
    /// atual nunca é a própria spec anterior, e a que divide uma palavra só
    /// fica fora.
    #[test]
    fn a_prior_spec_enters_as_facts_of_its_matching_rules_and_decisions() {
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
        let index = vec![line("antiga", "Travar o merge com pendência aberta."), line("atual", GOAL), line("fraca", "O merge da página.")];
        let prior = vec![("antiga".to_string(), old)];
        let list = build(&sources(&["fix"], None, &index, &prior));
        assert_eq!(list.len(), gaps(&["fix"]).len(), "{list:?}");
        assert_eq!(entered(&sources(&["fix"], None, &index, &prior)), ["antiga"]);
        let facts: Vec<(&str, &str)> = facts_of(&list, FACT_PRIOR_SPEC).iter().map(|f| (f.text.as_str(), f.source.as_str())).collect();
        assert!(facts.contains(&("Merge travado.", "mustard-rt run read agreed --spec antiga --term MSTD-RULE-0001")), "{facts:?}");
        assert!(facts.contains(&("A cobrança da pendência sai no merge.", "mustard-rt run read agreed --spec antiga --term MSTD-DEC-0001")), "{facts:?}");
        assert!(!facts.iter().any(|(text, _)| text.contains("título")), "{facts:?}");
    }

    /// As specs anteriores que entram no levantamento, na ordem, pelo nome
    /// que a fonte dos fatos de cada uma cita.
    fn entered(given: &Sources<'_>) -> Vec<String> {
        let name = |facts: &[Fact]| facts.first()?.source.split("--spec ").nth(1)?.split(' ').next().map(str::to_string);
        prior_spec_facts(given).iter().filter_map(|(_, facts)| name(facts)).collect()
    }

    /// A linha do índice de uma spec, com o objetivo no `search`.
    fn index_line(name: &str, goal: &str) -> IndexLine {
        IndexLine {
            name: name.to_string(),
            goal: Some(goal.to_string()),
            phase: Some("closed".to_string()),
            titles: Vec::new(),
            search: search_field(Some(goal), &[name]),
        }
    }

    /// Uma spec anterior que só traz a entrega de uma onda, com os arquivos.
    fn delivering(files: &[&str]) -> SpecLog {
        log_of(&[ev(1, "delivered", json!({"wave": 1, "text": "A onda saiu.", "files": files}))])
    }

    /// Com arquivo citado no objetivo, entram as specs anteriores que mexeram
    /// neles, sem precisar de palavra em comum: as que mexeram em mais
    /// arquivos primeiro e, no empate, pelo nome. O arquivo vem da entrega ou
    /// do que o item dela cita, no texto do item antigo ou na fonte de um
    /// fato. A spec que mexeu em outro arquivo, e a do índice sem arquivo de
    /// eventos lido, ficam de fora.
    #[test]
    fn an_earlier_spec_that_touched_the_cited_files_enters_the_most_shared_first() {
        let cites = log_of(&[ev(
            1,
            "point",
            json!({"block": "code", "gap": "Onde soma", "from": "gap", "status": "closed", "origin": 1,
                "facts": [{"text": "A soma.", "source": "src\\b.rs:4"}]}),
        )]);
        let old_rule = log_of(&[ev(1, "rule", json!({"text": "Vale em `src/a.rs`.", "keys": ["a"], "example": "e", "origin": 1}))]);
        let index = vec![
            index_line("um", "Reescrever a impressão."),
            index_line("dois", "Redesenhar o rótulo."),
            index_line("tres", "Trocar a cor."),
            index_line("quatro", "Travar o merge com pendência aberta."),
            index_line("cinco", "Mudar o menu."),
            index_line("sem-arquivo", "Mexer em tudo."),
            index_line("atual", GOAL),
        ];
        let prior = vec![
            ("um".to_string(), delivering(&["./src/a.rs"])),
            ("dois".to_string(), delivering(&["src/a.rs", "src/b.rs", "src/c.rs"])),
            ("tres".to_string(), cites),
            ("quatro".to_string(), delivering(&["src/c.rs"])),
            ("cinco".to_string(), old_rule),
        ];
        let mut given = sources(&["fix"], None, &index, &prior);
        given.goal_agent = "- o defeito está em `src/a.rs:3` e em `src/b.rs`.";
        assert_eq!(entered(&given), ["dois", "cinco", "tres", "um"]);
    }

    /// Os arquivos que a spec do levantamento cita, além dos do objetivo, vêm
    /// dos itens que o assistente gravou nela (parte do agente e fonte dos
    /// fatos). O ponto que o próprio `grill` gravou fica de fora: o que ele
    /// trouxe do mapa não faz outra spec entrar numa segunda chamada.
    #[test]
    fn the_survey_cites_the_files_of_its_own_items_but_not_of_the_points_the_grill_wrote() {
        let current = log_of(&[
            ev(1, "context", json!({"text": "Somar.", "agent": "- ver `src/x.rs`", "origin": 1})),
            ev(
                2,
                "point",
                json!({"author": "binary", "block": "code", "gap": "Onde soma", "from": "gap", "status": "open", "origin": 1,
                    "facts": [{"text": "Soma.", "source": "src/y.rs:1"}]}),
            ),
            ev(
                3,
                "point",
                json!({"author": "assistant", "block": "code", "gap": "Onde soma", "from": "gap", "status": "closed", "origin": 1,
                    "facts": [{"text": "Soma.", "source": "src/z.rs:1"}]}),
            ),
        ]);
        let index = vec![index_line("x", "Um."), index_line("y", "Dois."), index_line("z", "Três.")];
        let prior = vec![
            ("atual".to_string(), current),
            ("x".to_string(), delivering(&["src/x.rs"])),
            ("y".to_string(), delivering(&["src/y.rs"])),
            ("z".to_string(), delivering(&["src/z.rs"])),
        ];
        assert_eq!(entered(&sources(&["fix"], None, &index, &prior)), ["x", "z"]);
    }

    /// O usuário decide quem usa, as regras, os limites, os erros, os casos
    /// de borda, o que fica fora e o esperado contra o obtido; o assistente,
    /// o resto. O dono de um ponto gravado sai da lacuna, em qualquer idioma;
    /// o ponto que não casa lacuna nenhuma é do usuário.
    #[test]
    fn the_user_owns_only_the_product_gaps() {
        let user: Vec<GapKey> = GapKey::ALL.into_iter().filter(|key| key.owner() == Owner::User).collect();
        let product =
            [GapKey::WhoUses, GapKey::ExpectedVsActual, GapKey::Rules, GapKey::Limits, GapKey::Errors, GapKey::EdgeCases, GapKey::OutOfScope];
        assert_eq!(user, product);
        let point = |gap: &str| parse_log(&open_point(1, CONDENSED, gap)).get(1).cloned().unwrap();
        assert_eq!(owner_of(&point("Who uses it and what for")), Owner::User);
        assert_eq!(owner_of(&point("contracts")), Owner::Assistant);
        assert_eq!(owner_of(&point("Merge com pendência.")), Owner::User);
    }

    /// Todos os pontos vão para um bloco só, sem pedir.
    #[test]
    fn the_list_puts_every_point_in_one_block() {
        let list = build(&sources(&["feature", "fix"], None, &[], &[]));
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
        let whole = |_: Need<'_>| Ok(map.clone());
        given.map = Some(&whole);
        let list = build(&given);
        let dependents = list.iter().find(|p| p.key == GapKey::Dependents).unwrap();
        assert_eq!(
            dependents.facts,
            [
                Fact { text: "`record_birth` é declarado em src/a.rs, linha 3.".into(), source: "src/a.rs:3".into(), kind: FACT_CODE },
                Fact { text: "src/a.rs é importado por: src/b.rs, src/c.rs.".into(), source: "mustard-rt run map importers --file src/a.rs".into(), kind: FACT_CODE },
            ]
        );
        assert!(list.iter().filter(|p| p.key == GapKey::OutOfScope).all(|p| p.facts.is_empty()));
    }

    #[test]
    fn a_survey_recovers_consumers_contracts_and_test_candidates_without_deciding_product_scope() {
        let map: ProjectMap = serde_json::from_value(json!({"modules":[
            {"path":"src/billing.py","declarations":[{"kind":"function","name":"invoice","line":1}],
                "has_tests":false,"tests":["tests/test_billing.py"]},
            {"path":"src/api.py","deps":["src/billing.py"]}
        ]}))
        .unwrap();
        for lang in [Locale::PtBr, Locale::EnUs] {
            let mut given = sources(&["feature"], None, &[], &[]);
            given.lang = lang;
            given.goal_agent = "Change `invoice()`.";
            let whole = |_: Need<'_>| Ok(map.clone());
            given.map = Some(&whole);
            let list = build(&given);
            let uses = list.iter().find(|point| point.key == GapKey::WhoUses).unwrap();
            assert!(uses.facts.iter().any(|fact| fact.text.contains("src/api.py")));
            let contract = list.iter().find(|point| point.key == GapKey::Contracts).unwrap();
            assert_eq!(contract.facts[0].source, "src/billing.py:1");
            let proof = list.iter().find(|point| point.key == GapKey::DoneProof).unwrap();
            assert!(proof.facts.iter().any(|fact| fact.text.contains("tests/test_billing.py")));
            assert!(list.iter().filter(|point| point.key == GapKey::OutOfScope).all(|point| point.facts.is_empty()));
        }
    }

    /// O objetivo gravado em três partes não cita código na parte do usuário:
    /// o nome vem entre crases na parte do agente, e a lacuna de quem depende
    /// traz dele os mesmos fatos que traria do texto.
    #[test]
    fn the_name_cited_in_the_agent_part_of_the_goal_brings_its_declaration_and_who_imports_it() {
        let map: ProjectMap = serde_json::from_value(json!({
            "modules": [
                {"path": "src/a.rs", "declarations": [{"kind": "function", "name": "record_birth", "line": 3}]},
                {"path": "src/b.rs", "deps": ["src/a.rs"]},
                {"path": "src/c.rs", "deps": ["src/a.rs"]},
            ]
        }))
        .unwrap();
        let mut given = sources(&["refactor"], None, &[], &[]);
        given.goal = "Tirar o registro de nascimento do fluxo.";
        given.goal_agent = "- Tirar `record_birth` do fluxo.";
        let whole = |_: Need<'_>| Ok(map.clone());
        given.map = Some(&whole);
        let list = build(&given);
        let dependents = list.iter().find(|p| p.key == GapKey::Dependents).unwrap();
        assert_eq!(
            dependents.facts,
            [
                Fact { text: "`record_birth` é declarado em src/a.rs, linha 3.".into(), source: "src/a.rs:3".into(), kind: FACT_CODE },
                Fact { text: "src/a.rs é importado por: src/b.rs, src/c.rs.".into(), source: "mustard-rt run map importers --file src/a.rs".into(), kind: FACT_CODE },
            ]
        );
    }

    /// Um projeto com o nome do objetivo declarado em dois arquivos, cada um
    /// importado por outros, e um arquivo que o declara sem ninguém que o
    /// importe.
    const DEPENDENTS_MAP: &str = r#"{"modules": [
        {"path": "src/a.rs", "declarations": [{"kind": "function", "name": "record_birth", "line": 3}], "tests": ["tests/a.rs"]},
        {"path": "src/b.rs", "deps": ["src/a.rs", "src/z.rs"]},
        {"path": "src/c.rs", "deps": ["src/a.rs"]},
        {"path": "src/z.rs", "declarations": [{"kind": "function", "name": "record_birth", "line": 9}]},
        {"path": "src/d.rs", "declarations": [{"kind": "struct", "name": "Birth", "line": 1}]}
    ]}"#;

    /// A lacuna de quem depende, com o mapa gravado no disco e lido por
    /// `read`.
    fn dependents_facts(read: &MapReader<'_>) -> Vec<Fact> {
        let mut given = sources(&["refactor"], None, &[], &[]);
        given.goal = "Tirar o `record_birth` e o `Birth` do fluxo.";
        given.map = Some(read);
        let list = build(&given);
        list.iter().find(|p| p.key == GapKey::Dependents).unwrap().facts.clone()
    }

    /// O levantamento lê só as declarações com o nome e quem importa cada
    /// arquivo, e dá os mesmos fatos que dava com o mapa inteiro lido.
    #[test]
    fn the_survey_reading_only_its_tables_gives_the_facts_the_whole_map_gave() {
        let dir = tempfile::tempdir().unwrap();
        store::write_text(dir.path(), DEPENDENTS_MAP).unwrap();
        let by_question = dependents_facts(&|need| store::read_for(dir.path(), need));
        let whole = dependents_facts(&|_| store::read(dir.path()));
        assert_eq!(by_question, whole);
        assert_eq!(by_question.len(), 5, "{by_question:?}");
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(dependents_facts(&|need| store::read_for(empty.path(), need)), Vec::new());
    }

    /// Um mapa em que uma coluna que o levantamento não lê guarda o tipo
    /// errado: a leitura do mapa inteiro o recusa, e o levantamento, que lê
    /// só as declarações e quem importa, ainda dá os fatos.
    #[test]
    fn the_survey_answers_even_when_a_column_it_does_not_read_is_broken() {
        let dir = tempfile::tempdir().unwrap();
        store::write_text(dir.path(), &DEPENDENTS_MAP.replace(r#""tests": ["tests/a.rs"]"#, r#""tests": "um texto""#)).unwrap();
        assert!(store::read(dir.path()).is_err(), "the whole map refuses the broken column");
        let facts = dependents_facts(&|need| store::read_for(dir.path(), need));
        let sources: Vec<&str> = facts.iter().map(|fact| fact.source.as_str()).collect();
        assert_eq!(
            sources,
            ["src/a.rs:3", "mustard-rt run map importers --file src/a.rs", "src/z.rs:9", "mustard-rt run map importers --file src/z.rs", "src/d.rs:1",]
        );
    }

    /// Um ponto gravado registra o item da lista pela lacuna, com o rótulo em
    /// qualquer idioma ou pelo nome; o que falta continua na lista.
    #[test]
    fn a_recorded_gap_is_found_by_its_label_in_either_language_or_its_name() {
        let point = |id: u64, gap: &str| {
            ev(
                id,
                "point",
                json!({"block": "context", "gap": gap, "from": "gap", "status": "open", "origin": 1,
                "facts": [{"text": "f", "source": "mensagem 1"}]}),
            )
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
        let left: Vec<GapKey> = missing(&log, &list).iter().map(|p| p.key).collect();
        assert_eq!(left, [GapKey::Cause, GapKey::DoneProof]);
        assert_eq!(list[0].standing(&log), Some((log.get(3).unwrap(), "open")));
    }

    /// Um ponto aberto, como o assistente grava, com a mensagem 2 de origem.
    fn open_point(id: u64, block: &str, gap: &str) -> String {
        ev(
            id,
            "point",
            json!({"block": block, "gap": gap, "from": "gap", "status": "open", "origin": 2,
            "facts": [{"text": "f", "source": "mensagem 2"}]}),
        )
    }

    /// O fechamento do ponto `closes`.
    fn closing(id: u64, closes: u64) -> String {
        ev(
            id,
            "point",
            json!({"block": "b", "gap": "g", "from": "gap", "status": "closed", "closes": closes,
            "result": [2], "origin": 2}),
        )
    }

    /// O levantamento só sai com o tipo de trabalho gravado, um ponto para
    /// cada lacuna dele e nenhum ponto aberto, e diz o que falta nessa ordem.
    #[test]
    fn leaving_the_survey_names_what_still_holds_it() {
        let mut lines = vec![ev(1, "state", json!({"phase": "survey", "author": "binary"})), ev(2, "message", json!({"author": "user", "text": GOAL}))];
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

    /// O passo depois de cada gravação segue os pontos abertos: fechar um
    /// ponto traz o próximo, sem revisão do bloco; fechar o último traz o fim,
    /// sem revisor de fora. Depois da aprovação, nenhum passo.
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
        let step = next_step(&before, &after);
        assert!(matches!(step, Some(SurveyStep::Point(p)) if p.id == 3), "{step:?}");

        lines.push(closing(6, 3));
        let before = log_of(&lines[..lines.len() - 1]);
        let after = log_of(&lines);
        let step = next_step(&before, &after);
        assert!(matches!(step, Some(SurveyStep::Point(p)) if p.id == 4), "the block that closed asks no review: {step:?}");

        lines.push(closing(7, 4));
        let before = log_of(&lines[..lines.len() - 1]);
        let after = log_of(&lines);
        let step = next_step(&before, &after);
        assert!(matches!(&step, Some(SurveyStep::Done { unrouted }) if unrouted.is_empty()), "the end, with no reviewer: {step:?}");

        // Uma lacuna do tipo de trabalho ainda sem ponto: fechar o último
        // ponto aberto não acaba o levantamento.
        let mut unrecorded = lines[..4].to_vec();
        unrecorded.push(ev(8, "work_type", json!({"kinds": ["fix"], "origin": 2})));
        unrecorded.push(closing(9, 3));
        unrecorded.push(closing(10, 4));
        let after = log_of(&unrecorded);
        let step = next_step(&log_of(&unrecorded[..unrecorded.len() - 1]), &after);
        assert!(matches!(step, Some(SurveyStep::Record(_))), "a gap without a point holds the end: {step:?}");

        lines.push(ev(8, "state", json!({"phase": "approved", "author": "user"})));
        let before = log_of(&lines);
        lines.push(ev(9, "message", json!({"author": "user", "text": "Mais uma."})));
        assert!(next_step(&before, &log_of(&lines)).is_none(), "no step after the approval");
    }

    /// O ponto que o revisor de fora deixou numa spec antiga ainda é lido:
    /// aberto, ele é o próximo, depois dos blocos de antes; fechado, o
    /// levantamento chega ao fim.
    #[test]
    fn an_old_outside_review_point_is_still_read() {
        let mut lines = vec![
            ev(1, "state", json!({"phase": "survey", "author": "binary"})),
            ev(2, "message", json!({"author": "user", "text": GOAL})),
            ev(3, "point", json!({"block": "outside_review", "gap": "O merge pela web", "from": "outside_review",
                "status": "open", "origin": 2, "facts": [{"text": "f", "source": "mensagem 2"}]})),
            open_point(4, "rules", "Cada regra"),
        ];
        let log = log_of(&lines);
        assert_eq!(open_points(&log).iter().map(|p| p.id).collect::<Vec<_>>(), [4, 3], "the old block comes last");
        lines.push(closing(5, 4));
        let after = log_of(&lines);
        let step = next_step(&log_of(&lines[..lines.len() - 1]), &after);
        assert!(matches!(step, Some(SurveyStep::Point(p)) if p.id == 3), "{step:?}");
        lines.push(ev(6, "point", json!({"block": "outside_review", "gap": "O merge pela web", "from": "outside_review",
            "status": "closed", "closes": 3, "result": [2], "origin": 2})));
        let after = log_of(&lines);
        let step = next_step(&log_of(&lines[..lines.len() - 1]), &after);
        assert!(matches!(step, Some(SurveyStep::Done { .. })), "{step:?}");
        assert!(open_points(&after).is_empty());
    }
}
