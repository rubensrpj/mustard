//! `spec_events` — o arquivo de eventos de uma spec (`spec.ndjson`).
//!
//! Uma spec é um arquivo só, com um evento por linha. Este módulo guarda o que
//! vale para toda linha: os 33 tipos, o envelope, os campos obrigatórios de
//! cada tipo, o bloco de cada tipo e a leitura por bloco e por passo.
//!
//! Funciona porque os tipos são fixos: o tipo mora sempre no campo `type`,
//! cada tipo vai sempre para o mesmo bloco, e o gravador recusa tipo
//! desconhecido e campo obrigatório vazio. Ler um bloco é filtrar as linhas
//! pelo tipo e, numa onda, pelo número dela.
//!
//! Nada é apagado. `remove` tira itens da leitura e deixa as linhas no arquivo,
//! com o motivo; a versão nova de um item é um evento do mesmo tipo com
//! `replaces` apontando a antiga, que some da leitura. O expurgo é a exceção:
//! a linha do item fica só com o envelope, sem o texto.
//!
//! Cada item tem um código, `MSTD-<sigla>-<NNNN>`, que o binário grava na
//! linha: o maior número já dado ao tipo, mais 1. A versão nova de um item
//! grava o código da antiga. Como o código mora na linha, apagar outra linha à
//! mão não muda código nenhum, e um número que saiu não volta. Quem aponta um
//! item (`replaces`, os alvos de `remove` e `purge`) pode usar o número do
//! evento ou esse código.
//!
//! Função pura: sem disco e sem relógio. A trava, a gravação e o caminho do
//! arquivo moram em `io::spec_events`.

use std::collections::{BTreeMap, BTreeSet};

use rust_stemmers::{Algorithm, Stemmer};
use serde_json::{Map, Value};

use crate::domain::{mustard_id, text};
use crate::platform::i18n::{translate, Locale};

/// A versão do formato de cada linha. O leitor entende as anteriores.
pub const FORMAT_VERSION: u64 = 1;

/// Quem pode ter produzido um evento.
pub const AUTHORS: &[&str] = &["user", "assistant", "hook", "binary", "wave", "review", "skill"];

/// Quem grava pelo comando `write` sem dizer quem é: o assistente.
pub const DEFAULT_AUTHOR: &str = "assistant";

/// Os campos que só o binário escreve. O que vier neles de quem grava é
/// descartado e trocado, menos os de [`REFUSED_FIELDS`].
pub const BINARY_FIELDS: &[&str] = &["v", "id", "at", "search", "code"];

/// Os campos do binário que, vindos de quem grava, recusam o evento em vez de
/// serem descartados. Quem manda um código quer apontar um item, e trocar o
/// código em silêncio criaria um item novo no lugar: o item se aponta por
/// `replaces` ou pelos alvos de `remove` e `purge`.
pub const REFUSED_FIELDS: &[&str] = &["code"];

/// O campo que marca uma linha expurgada; guarda o número do expurgo.
pub const PURGED_FIELD: &str = "purged";

/// O envelope, na ordem em que abre cada linha do arquivo. O expurgo guarda o
/// envelope, então o código do item continua no arquivo depois dele.
const LEAD_FIELDS: &[&str] = &["v", "id", "code", "at", "type", "author"];

// ---------------------------------------------------------------------------
// Blocos
// ---------------------------------------------------------------------------

/// Os blocos da spec. Cada tipo de evento vai sempre para o mesmo bloco; o
/// painel de medição (`metrics`) é o único montado de tipos de outros blocos,
/// e ninguém grava nele.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Block {
    State,
    Metrics,
    Agreed,
    Specification,
    Criteria,
    Waves,
    Review,
    Progress,
    Notes,
    Conversation,
}

impl Block {
    /// Todos os blocos, na ordem da página.
    pub const ALL: [Self; 10] = [
        Self::State,
        Self::Metrics,
        Self::Agreed,
        Self::Specification,
        Self::Criteria,
        Self::Waves,
        Self::Review,
        Self::Progress,
        Self::Notes,
        Self::Conversation,
    ];

    /// O nome do bloco no `read`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::Metrics => "metrics",
            Self::Agreed => "agreed",
            Self::Specification => "specification",
            Self::Criteria => "criteria",
            Self::Waves => "waves",
            Self::Review => "review",
            Self::Progress => "progress",
            Self::Notes => "notes",
            Self::Conversation => "conversation",
        }
    }

    /// O bloco de um nome, ou `None` quando o nome não é de bloco nenhum.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.name() == name)
    }
}

/// Os tipos de que o painel de medição é montado: o texto colocado, os
/// bloqueios dos ganchos, as chamadas dos comandos, o tempo de cada fase, o
/// retrabalho, os lembretes do levantamento e o tamanho de cada pedido.
pub const METRIC_TYPES: &[&str] = &["injection", "hook", "call", "state", "verdict", "point", "send"];

/// O que o `read` pede: um bloco inteiro ou uma onda só (`wave-2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockQuery {
    Block(Block),
    Wave(u64),
}

impl BlockQuery {
    /// Lê `state`, `waves`, `wave-2`… ; `None` para um nome que não é bloco.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim();
        if let Some(n) = name.strip_prefix("wave-") {
            return n.parse().ok().map(Self::Wave);
        }
        Block::parse(name).map(Self::Block)
    }

    /// Os nomes aceitos, na ordem da página, para a mensagem de recusa.
    #[must_use]
    pub fn accepted_names() -> String {
        let mut names: Vec<&str> = Block::ALL.iter().map(|b| b.name()).collect();
        if let Some(i) = names.iter().position(|n| *n == "waves") {
            names.insert(i + 1, "wave-<n>");
        }
        names.join(", ")
    }
}

// ---------------------------------------------------------------------------
// Os campos
// ---------------------------------------------------------------------------

/// A forma de um campo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Int,
    Bool,
    Object,
    /// Uma lista de números: de eventos, de ondas.
    Ints,
    /// Uma lista de textos: caminhos, palavras-chave.
    Texts,
    /// Uma lista de objetos.
    Objects,
    /// Uma lista de qualquer coisa.
    List,
    /// Uma destas palavras.
    OneOf(&'static [&'static str]),
    /// Uma lista só com estas palavras.
    ManyOf(&'static [&'static str]),
    /// Um texto ou um objeto.
    TextOrObject,
    /// Uma data e hora como `2026-09-11T21:03`.
    Time,
    /// Um evento apontado: o número dele ou o código do item.
    Ref,
    /// Uma lista de eventos apontados, cada um pelo número ou pelo código.
    Refs,
}

impl Kind {
    pub(crate) fn accepts(self, value: &Value) -> bool {
        let is_int = |v: &Value| v.is_i64() || v.is_u64();
        let is_ref = |v: &Value| EventRef::from_value(v).is_some();
        match self {
            Self::Ref => is_ref(value),
            Self::Refs => value.as_array().is_some_and(|a| a.iter().all(is_ref)),
            Self::Text => value.is_string(),
            Self::Int => is_int(value),
            Self::Bool => value.is_boolean(),
            Self::Object => value.is_object(),
            Self::Ints => value.as_array().is_some_and(|a| a.iter().all(is_int)),
            Self::Texts => value.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
            Self::Objects => value.as_array().is_some_and(|a| a.iter().all(Value::is_object)),
            Self::List => value.is_array(),
            Self::OneOf(words) => value.as_str().is_some_and(|s| words.contains(&s)),
            Self::ManyOf(words) => value
                .as_array()
                .is_some_and(|a| a.iter().all(|v| v.as_str().is_some_and(|s| words.contains(&s)))),
            Self::TextOrObject => value.is_string() || value.is_object(),
            Self::Time => value.as_str().is_some_and(is_time_prefix),
        }
    }

    /// A forma em palavras, para a recusa.
    #[must_use]
    pub fn describe(self, lang: Locale) -> String {
        let (key, words) = match self {
            Self::Text => ("spec_events.kind.text", None),
            Self::Int => ("spec_events.kind.int", None),
            Self::Bool => ("spec_events.kind.bool", None),
            Self::Object => ("spec_events.kind.object", None),
            Self::Ints => ("spec_events.kind.ints", None),
            Self::Texts => ("spec_events.kind.texts", None),
            Self::Objects => ("spec_events.kind.objects", None),
            Self::List => ("spec_events.kind.list", None),
            Self::OneOf(w) => ("spec_events.kind.one_of", Some(w)),
            Self::ManyOf(w) => ("spec_events.kind.many_of", Some(w)),
            Self::TextOrObject => ("spec_events.kind.text_or_object", None),
            Self::Time => ("spec_events.kind.time", None),
            Self::Ref => ("spec_events.kind.ref", None),
            Self::Refs => ("spec_events.kind.refs", None),
        };
        let base = translate(key, lang);
        words.map_or_else(|| base.to_string(), |w| base.replace("{values}", &w.join(", ")))
    }
}

/// `2026-09-11`, `2026-09-11T21:03` ou `2026-09-11T21:03:12`: o começo de um
/// `at`, na hora local de quem lê.
fn is_time_prefix(s: &str) -> bool {
    const SHAPE: &[u8] = b"dddd-dd-ddTdd:dd:dd";
    let b = s.as_bytes();
    matches!(b.len(), 10 | 16 | 19)
        && b.iter().zip(SHAPE).all(|(c, s)| if *s == b'd' { c.is_ascii_digit() } else { c == s })
}

/// Como um campo aponta outro evento: pelo número do evento ou pelo código do
/// item, o mesmo que a página mostra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventRef {
    Id(u64),
    Code(String),
}

impl EventRef {
    /// O evento apontado por um valor: um número positivo ou um código
    /// inteiro, como `MSTD-RULE-0002`. `None` para qualquer outra coisa.
    #[must_use]
    pub fn from_value(value: &Value) -> Option<Self> {
        if let Some(id) = value.as_u64() {
            return Some(Self::Id(id));
        }
        let code = value.as_str()?.trim();
        mustard_id::is_id(code).then(|| Self::Code(code.to_string()))
    }
}

/// Um campo de um tipo.
#[derive(Debug, Clone, Copy)]
pub struct Field {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
}

pub(crate) const fn req(name: &'static str, kind: Kind) -> Field {
    Field { name, kind, required: true }
}

pub(crate) const fn opt(name: &'static str, kind: Kind) -> Field {
    Field { name, kind, required: false }
}

/// Um tipo de evento: o nome, a sigla do código, o bloco, e os campos
/// próprios.
#[derive(Debug)]
pub struct TypeSpec {
    pub name: &'static str,
    /// A sigla do tipo no código de cada item, `MSTD-<sigla>-<NNNN>`: só
    /// letras maiúsculas, uma por tipo.
    pub code: &'static str,
    pub block: Block,
    /// O assistente grava este tipo a partir da conversa: o número da mensagem
    /// de onde ele veio (`origin`) é obrigatório.
    pub needs_origin: bool,
    pub fields: &'static [Field],
}

const fn ty(
    name: &'static str,
    code: &'static str,
    block: Block,
    needs_origin: bool,
    fields: &'static [Field],
) -> TypeSpec {
    TypeSpec { name, code, block, needs_origin, fields }
}

const TEXT: Field = req("text", Kind::Text);
const KEYS: Field = req("keys", Kind::Texts);
const APPLIES_TO: Field = opt("applies_to", Kind::TextOrObject);

const HOOK_ACTIONS: &[&str] = &["warn", "block"];
const CALL_RESULTS: &[&str] = &["ok", "refused"];
/// As fases de uma spec, na ordem em que acontecem.
pub const PHASES: &[&str] =
    &["survey", "plan", "approved", "running", "closed", "pr_open", "delivered", "discarded"];
const PAGES: &[&str] = &["spec"];
const MILESTONES: &[&str] = &["approval", "round", "close"];
const WORK_KINDS: &[&str] = &["feature", "fix", "refactor"];
const POINT_FROM: &[&str] = &["gap", "lesson", "prior_spec", "code_conflict", "outside_review"];
const POINT_STATUS: &[&str] = &["open", "closed", "not_applicable"];
const RUN_RESULTS: &[&str] = &["pass", "fail"];
const SKILL_ACTIONS: &[&str] = &["create", "change", "drop"];
const ROLES: &[&str] = &["wave", "review", "skill"];
const VERDICTS: &[&str] = &["approved", "rejected"];
const EFFECTS: &[&str] = &["new_waves", "adjust_waves"];
const PURGE_REASONS: &[&str] = &["secret", "client_data"];

/// Os 33 tipos. Os campos marcados com `opt` podem faltar; os outros são
/// obrigatórios, e o gravador recusa o evento sem eles.
pub const TYPES: &[TypeSpec] = &[
    // Conversa.
    ty("message", "MSG", Block::Conversation, false, &[TEXT]),
    ty("response", "RESP", Block::Conversation, false, &[TEXT, req("reply_to", Kind::Int)]),
    ty(
        "injection",
        "INJ",
        Block::Conversation,
        false,
        &[req("hook", Kind::Text), req("chars", Kind::Int), TEXT],
    ),
    ty(
        "hook",
        "HOOK",
        Block::Conversation,
        false,
        &[
            req("hook", Kind::Text),
            req("action", Kind::OneOf(HOOK_ACTIONS)),
            req("tool", Kind::Text),
            req("reason", Kind::Text),
        ],
    ),
    ty(
        "call",
        "CALL",
        Block::Conversation,
        false,
        &[
            req("command", Kind::Text),
            req("ms", Kind::Int),
            req("result", Kind::OneOf(CALL_RESULTS)),
            opt("refusal", Kind::Text),
        ],
    ),
    // Estado.
    ty(
        "state",
        "STATE",
        Block::State,
        false,
        &[
            req("phase", Kind::OneOf(PHASES)),
            opt("branch", Kind::Text),
            opt("base", Kind::Text),
            opt("witness", Kind::Object),
            opt("pr", Kind::Object),
            opt("reason", Kind::Text),
        ],
    ),
    ty(
        "publish",
        "PUB",
        Block::State,
        false,
        &[
            req("page", Kind::OneOf(PAGES)),
            req("milestone", Kind::OneOf(MILESTONES)),
            req("ok", Kind::Bool),
            opt("url", Kind::Text),
            opt("reason", Kind::Text),
        ],
    ),
    // Combinado.
    ty("work_type", "WORK", Block::Agreed, true, &[req("kinds", Kind::ManyOf(WORK_KINDS))]),
    ty(
        "point",
        "POINT",
        Block::Agreed,
        true,
        &[
            req("block", Kind::Text),
            req("gap", Kind::Text),
            req("from", Kind::OneOf(POINT_FROM)),
            req("status", Kind::OneOf(POINT_STATUS)),
            opt("facts", Kind::Objects),
            opt("closes", Kind::Int),
            opt("result", Kind::Ints),
            opt("reason", Kind::Text),
            opt("reminders", Kind::List),
        ],
    ),
    ty("rule", "RULE", Block::Agreed, true, &[TEXT, KEYS, req("example", Kind::Text), APPLIES_TO]),
    ty("limit", "LIMIT", Block::Agreed, true, &[TEXT, KEYS, req("value", Kind::Text), APPLIES_TO]),
    ty("contract", "CONTR", Block::Agreed, true, &[TEXT, KEYS, req("example", Kind::Text), APPLIES_TO]),
    ty("error", "ERR", Block::Agreed, true, &[TEXT, KEYS, req("message", Kind::Text)]),
    ty("edge_case", "EDGE", Block::Agreed, true, &[TEXT, KEYS, req("expected", Kind::Text)]),
    ty("out_of_scope", "SCOPE", Block::Agreed, true, &[TEXT, KEYS, opt("reason", Kind::Text)]),
    ty("decision", "DEC", Block::Agreed, true, &[TEXT, KEYS, req("why", Kind::Text)]),
    // Especificação.
    ty("context", "CTX", Block::Specification, true, &[TEXT]),
    ty("concern", "CONC", Block::Specification, true, &[TEXT]),
    // Critérios.
    ty(
        "criterion",
        "CRIT",
        Block::Criteria,
        true,
        &[
            req("when", Kind::Text),
            req("then", Kind::Text),
            req("proof", Kind::Text),
            opt("contracts", Kind::Ints),
        ],
    ),
    ty(
        "criterion_run",
        "CRUN",
        Block::Criteria,
        false,
        &[
            req("criterion", Kind::Int),
            req("result", Kind::OneOf(RUN_RESULTS)),
            req("exit", Kind::Int),
            req("ms", Kind::Int),
            opt("output", Kind::Text),
        ],
    ),
    // Ondas.
    ty(
        "wave",
        "WAVE",
        Block::Waves,
        true,
        &[
            req("n", Kind::Int),
            TEXT,
            req("criteria", Kind::Ints),
            req("done_when", Kind::Text),
            opt("depends_on", Kind::Ints),
        ],
    ),
    ty(
        "task",
        "TASK",
        Block::Waves,
        true,
        &[
            req("wave", Kind::Int),
            TEXT,
            // Nem toda tarefa muda um arquivo que já se sabe qual é: a
            // tarefa sem arquivo fica sem o campo.
            opt("files", Kind::Objects),
            opt("skill", Kind::Text),
            opt("covers", Kind::Ints),
            opt("must_read", Kind::List),
        ],
    ),
    ty(
        "skill",
        "SKILL",
        Block::Waves,
        false,
        &[
            req("name", Kind::Text),
            req("action", Kind::OneOf(SKILL_ACTIONS)),
            TEXT,
            req("sha", Kind::Text),
            opt("examples", Kind::Objects),
        ],
    ),
    ty(
        "send",
        "SEND",
        Block::Waves,
        false,
        &[
            req("wave", Kind::Int),
            req("role", Kind::OneOf(ROLES)),
            req("lines", Kind::Int),
            req("chars", Kind::Int),
            req("items", Kind::Ints),
            req("mustard", Kind::Text),
            opt("lessons", Kind::Ints),
            opt("skills", Kind::Objects),
        ],
    ),
    ty(
        "delivered",
        "DELIV",
        Block::Waves,
        false,
        &[req("wave", Kind::Int), TEXT, req("files", Kind::Texts)],
    ),
    // Revisão.
    ty(
        "verdict",
        "VERD",
        Block::Review,
        false,
        &[
            req("wave", Kind::Int),
            req("result", Kind::OneOf(VERDICTS)),
            TEXT,
            req("criteria", Kind::Objects),
            opt("lessons", Kind::Objects),
        ],
    ),
    // Andamento.
    ty(
        "commit",
        "COMMIT",
        Block::Progress,
        false,
        &[
            req("sha", Kind::Text),
            req("title", Kind::Text),
            req("waves", Kind::Ints),
            req("files", Kind::Texts),
            req("repo", Kind::Text),
        ],
    ),
    ty("pr_summary", "PRSUM", Block::Progress, false, &[TEXT]),
    // Anotações.
    ty("request", "REQ", Block::Notes, true, &[TEXT, KEYS, req("effect", Kind::OneOf(EFFECTS))]),
    ty("deferred", "DEFER", Block::Notes, true, &[TEXT, KEYS, req("pending", Kind::Int)]),
    ty("note", "NOTE", Block::Notes, true, &[TEXT, KEYS]),
    // Remoção e expurgo, na conversa.
    ty(
        "remove",
        "RMV",
        Block::Conversation,
        false,
        &[req("reason", Kind::Text), opt("targets", Kind::Refs), opt("filter", Kind::Object)],
    ),
    ty(
        "purge",
        "PURGE",
        Block::Conversation,
        false,
        &[req("targets", Kind::Refs), req("reason", Kind::OneOf(PURGE_REASONS))],
    ),
];

/// Os objetos com campos obrigatórios próprios, dentro de uma lista ou de um
/// campo: o tipo, o campo e os campos de dentro. A fonte de cada fato do ponto
/// tem recusa própria e não entra aqui.
const NESTED: &[(&str, &str, &[&str])] = &[
    ("point", "facts", &["text"]),
    ("task", "files", &["path"]),
    ("skill", "examples", &["path", "why"]),
    ("send", "skills", &["name", "sha"]),
    ("verdict", "criteria", &["criterion", "tests_rule"]),
    ("verdict", "lessons", &["lesson", "repeated"]),
    ("state", "witness", &["question", "answer"]),
    ("remove", "filter", &["type", "from", "to"]),
];

/// O tipo pelo nome.
#[must_use]
pub fn type_spec(name: &str) -> Option<&'static TypeSpec> {
    TYPES.iter().find(|t| t.name == name)
}

/// Os nomes dos tipos, separados por vírgula, para a recusa.
#[must_use]
pub fn type_names() -> String {
    TYPES.iter().map(|t| t.name).collect::<Vec<_>>().join(", ")
}

// ---------------------------------------------------------------------------
// Recusas
// ---------------------------------------------------------------------------

/// Por que um evento ou uma lição não foi gravado, ou um bloco não foi lido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotAnObject { detail: String },
    UnknownType { found: String },
    MissingField { event_type: String, field: String },
    InvalidValue { event_type: String, field: String, expected: Kind },
    WrongCount { event_type: String, field: String, min: usize, max: usize, count: usize },
    FactWithoutSource { fact: usize },
    CitedFileMissing { fact: usize, path: String },
    CitedLineMissing { fact: usize, path: String, line: u64, lines: u64 },
    BinaryOnlyField { field: String },
    UnknownTarget { target: EventRef },
    ReplacesOtherType { id: u64, found: String, event_type: String },
    FilterMatchesNothing { event_type: String, from: String, to: String },
    UnknownBlock { found: String },
    BadSpecName { spec: String },
    NoSpecFile { spec: String },
    /// Uma leitura sem `--spec`, e nem a variável de ambiente, nem a branch,
    /// nem a sessão apontam uma spec.
    NoCurrentSpec,
    /// Um tipo de evento da spec gravado sem dizer a spec.
    SpecRequired { event_type: String },
    /// A lição que `replaces` aponta não existe no banco de lições.
    UnknownLesson { id: u64 },
    /// A lição não diz onde nasceu.
    LessonOriginMissing,
    /// Uma gravação que muda a fase da spec por uma porta que não grava essa
    /// mudança: a regra única da mudança de fase recusou.
    PhaseChangeRefused { spec: String, from: String, to: String },
    /// O `run write` com o tipo `state`, ou uma gravação dele que mudaria o
    /// estado: o estado é gravado pelos comandos do fluxo e pela testemunha.
    StateByFlowOnly { spec: String },
    /// O `run write` com um tipo que só o binário grava (a execução de um
    /// critério e o veredito da revisão), ou uma gravação dele que tiraria ou
    /// reveria um desses eventos.
    BinaryOnlyType { event_type: String, spec: String },
    /// A página pedida de uma spec cujo `spec.md` é o documento do
    /// `spec-draft`, que refazer do arquivo de eventos apagaria.
    DraftedSpec { spec: String },
    Io { detail: String },
}

impl Refusal {
    /// A razão curta, estável, para quem lê a saída por máquina.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::NotAnObject { .. } => "not-an-object",
            Self::UnknownType { .. } => "unknown-type",
            Self::MissingField { .. } => "missing-field",
            Self::InvalidValue { .. } => "invalid-value",
            Self::WrongCount { .. } => "wrong-count",
            Self::FactWithoutSource { .. } => "fact-without-source",
            Self::CitedFileMissing { .. } => "cited-file-missing",
            Self::CitedLineMissing { .. } => "cited-line-missing",
            Self::BinaryOnlyField { .. } => "binary-only-field",
            Self::UnknownTarget { .. } => "unknown-target",
            Self::ReplacesOtherType { .. } => "replaces-other-type",
            Self::FilterMatchesNothing { .. } => "filter-matches-nothing",
            Self::UnknownBlock { .. } => "unknown-block",
            Self::BadSpecName { .. } => "bad-spec-name",
            Self::NoSpecFile { .. } => "no-spec-file",
            Self::NoCurrentSpec => "no-current-spec",
            Self::SpecRequired { .. } => "spec-required",
            Self::UnknownLesson { .. } => "unknown-lesson",
            Self::LessonOriginMissing => "lesson-origin-missing",
            Self::PhaseChangeRefused { .. } => "phase-change-refused",
            Self::StateByFlowOnly { .. } => "state-by-flow-only",
            Self::BinaryOnlyType { .. } => "binary-only-type",
            Self::DraftedSpec { .. } => "drafted-spec",
            Self::Io { .. } => "io-failed",
        }
    }

    /// A mensagem exata, no idioma pedido.
    #[must_use]
    pub fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, String)]| {
            slots
                .iter()
                .fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::NotAnObject { detail } => {
                fill("spec_events.not_an_object", &[("{detail}", detail.clone())])
            }
            Self::UnknownType { found } => fill(
                "spec_events.unknown_type",
                &[("{type}", found.clone()), ("{types}", type_names())],
            ),
            Self::MissingField { event_type, field } => fill(
                "spec_events.missing_field",
                &[("{type}", event_type.clone()), ("{field}", field.clone())],
            ),
            Self::InvalidValue { event_type, field, expected } => fill(
                "spec_events.invalid_value",
                &[
                    ("{type}", event_type.clone()),
                    ("{field}", field.clone()),
                    ("{expected}", expected.describe(lang)),
                ],
            ),
            Self::WrongCount { event_type, field, min, max, count } => fill(
                "spec_events.wrong_count",
                &[
                    ("{type}", event_type.clone()),
                    ("{field}", field.clone()),
                    ("{min}", min.to_string()),
                    ("{max}", max.to_string()),
                    ("{count}", count.to_string()),
                ],
            ),
            Self::FactWithoutSource { fact } => {
                fill("spec_events.fact_without_source", &[("{fact}", fact.to_string())])
            }
            Self::CitedFileMissing { fact, path } => fill(
                "spec_events.cited_file_missing",
                &[("{fact}", fact.to_string()), ("{path}", path.clone())],
            ),
            Self::CitedLineMissing { fact, path, line, lines } => fill(
                "spec_events.cited_line_missing",
                &[
                    ("{fact}", fact.to_string()),
                    ("{path}", path.clone()),
                    ("{line}", line.to_string()),
                    ("{lines}", lines.to_string()),
                ],
            ),
            Self::BinaryOnlyField { field } => {
                fill("spec_events.binary_only_field", &[("{field}", field.clone())])
            }
            Self::UnknownTarget { target: EventRef::Id(id) } => {
                fill("spec_events.unknown_target", &[("{id}", id.to_string())])
            }
            Self::UnknownTarget { target: EventRef::Code(code) } => {
                fill("spec_events.unknown_code", &[("{code}", code.clone())])
            }
            Self::ReplacesOtherType { id, found, event_type } => fill(
                "spec_events.replaces_other_type",
                &[
                    ("{id}", id.to_string()),
                    ("{found}", found.clone()),
                    ("{type}", event_type.clone()),
                ],
            ),
            Self::FilterMatchesNothing { event_type, from, to } => fill(
                "spec_events.filter_matches_nothing",
                &[("{type}", event_type.clone()), ("{from}", from.clone()), ("{to}", to.clone())],
            ),
            Self::UnknownBlock { found } => fill(
                "spec_events.unknown_block",
                &[("{block}", found.clone()), ("{blocks}", BlockQuery::accepted_names())],
            ),
            Self::BadSpecName { spec } => {
                fill("spec_events.bad_spec_name", &[("{spec}", spec.clone())])
            }
            Self::NoSpecFile { spec } => {
                fill("spec_events.no_spec_file", &[("{spec}", spec.clone())])
            }
            Self::NoCurrentSpec => fill("spec_events.no_current_spec", &[]),
            Self::SpecRequired { event_type } => {
                fill("spec_events.spec_required", &[("{type}", event_type.clone())])
            }
            Self::UnknownLesson { id } => fill("lessons.unknown_lesson", &[("{id}", id.to_string())]),
            Self::LessonOriginMissing => fill("lessons.origin_missing", &[]),
            Self::PhaseChangeRefused { spec, from, to } => fill(
                "spec_events.phase_change_refused",
                &[("{spec}", spec.clone()), ("{from}", from.clone()), ("{to}", to.clone())],
            ),
            Self::DraftedSpec { spec } => fill("spec_events.drafted_spec", &[("{spec}", spec.clone())]),
            Self::StateByFlowOnly { spec } => {
                fill("spec_events.state_by_flow_only", &[("{spec}", spec.clone())])
            }
            Self::BinaryOnlyType { event_type, spec } => fill(
                "spec_events.binary_only_type",
                &[("{type}", event_type.clone()), ("{spec}", spec.clone())],
            ),
            Self::Io { detail } => fill("spec_events.io_failed", &[("{detail}", detail.clone())]),
        }
    }
}

// ---------------------------------------------------------------------------
// Gravação: preparar, conferir, carimbar e escrever a linha
// ---------------------------------------------------------------------------

/// `true` para o que não vale como valor: ausente, `null`, texto em branco,
/// lista vazia ou objeto vazio. Número e `true`/`false` nunca são vazios.
pub(crate) fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        Value::Bool(_) | Value::Number(_) => false,
    }
}

/// O rascunho de quem grava, pronto para a conferência: sem os campos que só
/// o binário escreve, com o tipo pedido e com o autor (o assistente, quando
/// quem grava não diz). Os campos de [`REFUSED_FIELDS`] ficam, para que
/// [`validate`] recuse o evento que os trouxe.
#[must_use]
pub fn normalize(mut draft: Map<String, Value>, event_type: &str) -> Map<String, Value> {
    for field in BINARY_FIELDS.iter().filter(|f| !REFUSED_FIELDS.contains(f)) {
        draft.remove(*field);
    }
    draft.remove(PURGED_FIELD);
    draft.insert("type".into(), Value::String(event_type.trim().to_string()));
    if draft.get("author").is_none_or(is_empty) {
        draft.insert("author".into(), Value::String(DEFAULT_AUTHOR.into()));
    }
    draft
}

/// Confere um evento sozinho: o tipo existe, o autor é conhecido, cada campo
/// obrigatório está preenchido e cada campo tem a forma certa. O que depende
/// do resto do arquivo (o número apontado existe, o arquivo citado existe)
/// fica para [`check_against`] e para o gravador.
pub fn validate(event: &Map<String, Value>) -> Result<(), Refusal> {
    let found = event.get("type").and_then(Value::as_str).unwrap_or_default();
    let Some(spec) = type_spec(found) else {
        return Err(Refusal::UnknownType { found: found.to_string() });
    };
    if let Some(field) = REFUSED_FIELDS.iter().find(|f| event.contains_key(**f)) {
        return Err(Refusal::BinaryOnlyField { field: (*field).to_string() });
    }
    check_field(event, spec.name, req("author", Kind::OneOf(AUTHORS)))?;
    check_field(event, spec.name, Field { name: "origin", kind: Kind::Int, required: spec.needs_origin })?;
    for envelope in [opt("label", Kind::Text), opt("replaces", Kind::Ref)] {
        check_field(event, spec.name, envelope)?;
    }
    for shared in [opt("text", Kind::Text), opt("keys", Kind::Texts)] {
        if !spec.fields.iter().any(|f| f.name == shared.name) {
            check_field(event, spec.name, shared)?;
        }
    }
    for field in spec.fields {
        check_field(event, spec.name, *field)?;
    }
    check_nested(event, spec.name)?;
    check_conditions(event, spec.name)?;
    check_fact_sources(event, spec.name)
}

pub(crate) fn check_field(event: &Map<String, Value>, event_type: &str, field: Field) -> Result<(), Refusal> {
    match event.get(field.name) {
        Some(value) if !is_empty(value) => {
            if field.kind.accepts(value) {
                Ok(())
            } else {
                Err(Refusal::InvalidValue {
                    event_type: event_type.to_string(),
                    field: field.name.to_string(),
                    expected: field.kind,
                })
            }
        }
        _ if field.required => Err(missing(event_type, field.name)),
        _ => Ok(()),
    }
}

fn missing(event_type: &str, field: &str) -> Refusal {
    Refusal::MissingField { event_type: event_type.to_string(), field: field.to_string() }
}

fn check_nested(event: &Map<String, Value>, event_type: &str) -> Result<(), Refusal> {
    for (owner, field, inner) in NESTED {
        if *owner != event_type {
            continue;
        }
        match event.get(*field) {
            Some(Value::Array(items)) => {
                for (i, item) in items.iter().enumerate() {
                    let Some(obj) = item.as_object() else { continue };
                    for key in *inner {
                        if obj.get(*key).is_none_or(is_empty) {
                            return Err(missing(event_type, &format!("{field}[{}].{key}", i + 1)));
                        }
                    }
                }
            }
            Some(Value::Object(obj)) => {
                for key in *inner {
                    if obj.get(*key).is_none_or(is_empty) {
                        return Err(missing(event_type, &format!("{field}.{key}")));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Os campos que só são obrigatórios numa situação: a testemunha na
/// aprovação, o motivo no descarte, o endereço da publicação que deu certo, a
/// fonte dos fatos do ponto aberto, os exemplos da skill que nasce, o alvo da
/// remoção.
fn check_conditions(event: &Map<String, Value>, event_type: &str) -> Result<(), Refusal> {
    let has = |f: &str| event.get(f).is_some_and(|v| !is_empty(v));
    let need = |f: &str| if has(f) { Ok(()) } else { Err(missing(event_type, f)) };
    let word = |f: &str| event.get(f).and_then(Value::as_str).unwrap_or_default();
    match event_type {
        "state" => match word("phase") {
            "approved" => need("witness"),
            "discarded" => need("reason"),
            "pr_open" => need("pr"),
            _ => Ok(()),
        },
        "publish" => {
            if event.get("ok").and_then(Value::as_bool) == Some(true) {
                need("url")
            } else {
                need("reason")
            }
        }
        "point" => {
            let reminders = event.get("reminders").and_then(Value::as_array).map_or(0, Vec::len);
            if reminders > 3 {
                return Err(Refusal::WrongCount {
                    event_type: event_type.to_string(),
                    field: "reminders".into(),
                    min: 0,
                    max: 3,
                    count: reminders,
                });
            }
            if word("status") == "open" {
                return need("facts");
            }
            need("closes")?;
            if has("result") || has("reason") { Ok(()) } else { Err(missing(event_type, "result")) }
        }
        "skill" if word("action") == "create" => {
            need("examples")?;
            let count = event.get("examples").and_then(Value::as_array).map_or(0, Vec::len);
            if (2..=3).contains(&count) {
                Ok(())
            } else {
                Err(Refusal::WrongCount {
                    event_type: event_type.to_string(),
                    field: "examples".into(),
                    min: 2,
                    max: 3,
                    count,
                })
            }
        }
        "remove" => {
            if !has("targets") && !has("filter") {
                return Err(missing(event_type, "targets"));
            }
            let Some(filter) = event.get("filter").and_then(Value::as_object) else {
                return Ok(());
            };
            let filtered = filter.get("type").and_then(Value::as_str).unwrap_or_default();
            if type_spec(filtered).is_none() {
                return Err(Refusal::UnknownType { found: filtered.to_string() });
            }
            for bound in ["from", "to"] {
                if !filter.get(bound).is_some_and(|v| Kind::Time.accepts(v)) {
                    return Err(Refusal::InvalidValue {
                        event_type: event_type.to_string(),
                        field: format!("filter.{bound}"),
                        expected: Kind::Time,
                    });
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Todo fato de um ponto leva a fonte: o arquivo e a linha, o comando com o
/// resultado, ou o número da mensagem do usuário.
fn check_fact_sources(event: &Map<String, Value>, event_type: &str) -> Result<(), Refusal> {
    if event_type != "point" {
        return Ok(());
    }
    let facts = event.get("facts").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    for (i, fact) in facts.iter().enumerate() {
        if fact.get("source").is_none_or(is_empty) {
            return Err(Refusal::FactWithoutSource { fact: i + 1 });
        }
    }
    Ok(())
}

/// A fonte de um fato que cita um arquivo: `caminho:linha` ou
/// `caminho:início-fim`, sem espaço. Devolve o caminho, com barras normais, e
/// a última linha citada. `None` para as outras fontes — um comando com o
/// resultado, o número de uma mensagem, um endereço.
#[must_use]
pub fn file_citation(source: &str) -> Option<(String, u64)> {
    let source = source.trim();
    if source.is_empty() || source.contains(char::is_whitespace) || source.contains("://") {
        return None;
    }
    let (path, lines) = source.rsplit_once(':')?;
    let (start, end) = lines.split_once('-').unwrap_or((lines, lines));
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(start) || !digits(end) {
        return None;
    }
    let path = path.replace('\\', "/");
    if path.is_empty() || !(path.contains('/') || path.contains('.')) {
        return None;
    }
    let start: u64 = start.parse().ok()?;
    let end: u64 = end.parse().ok()?;
    Some((path, start.max(end)))
}

/// Troca cada código (`MSTD-RULE-0002`) dos campos que apontam eventos pelos
/// números que ele nomeia no arquivo como está, para que a linha gravada
/// guarde só números: em `replaces`, a versão mais nova do item; nos alvos de
/// `remove` e `purge`, todas as versões dele. Um código que não existe na spec
/// recusa o evento. Um evento sem código nenhum sai como entrou.
pub fn resolve_codes(log: &SpecLog, event: &mut Map<String, Value>) -> Result<(), Refusal> {
    let is_code = |v: &Value| matches!(EventRef::from_value(v), Some(EventRef::Code(_)));
    let replaces_code = event.get("replaces").is_some_and(is_code);
    let targets = event.get("targets").and_then(Value::as_array).cloned().unwrap_or_default();
    if !replaces_code && !targets.iter().any(is_code) {
        return Ok(());
    }
    let codes = log.codes();
    let ids_of = |code: &str| -> Result<Vec<u64>, Refusal> {
        let ids: Vec<u64> =
            log.events.iter().map(|e| e.id).filter(|id| codes.get(id).is_some_and(|c| c == code)).collect();
        if ids.is_empty() {
            Err(Refusal::UnknownTarget { target: EventRef::Code(code.to_string()) })
        } else {
            Ok(ids)
        }
    };
    if let Some(EventRef::Code(code)) = event.get("replaces").and_then(EventRef::from_value) {
        let newest = ids_of(&code)?.last().copied().unwrap_or_default();
        event.insert("replaces".into(), Value::from(newest));
    }
    if targets.iter().any(is_code) {
        let mut resolved: Vec<Value> = Vec::new();
        for target in &targets {
            let ids: Vec<Value> = match EventRef::from_value(target) {
                Some(EventRef::Code(code)) => ids_of(&code)?.into_iter().map(Value::from).collect(),
                _ => vec![target.clone()],
            };
            for id in ids {
                if !resolved.contains(&id) {
                    resolved.push(id);
                }
            }
        }
        event.insert("targets".into(), Value::Array(resolved));
    }
    Ok(())
}

/// O que uma gravação muda no resto do arquivo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Effects {
    /// Os números que um `remove` tira da leitura.
    pub removed: Vec<u64>,
    /// Os números cujo texto um `purge` tira do arquivo.
    pub purged: Vec<u64>,
}

/// Confere o evento contra o arquivo como está: o número que `replaces`
/// aponta existe e é do mesmo tipo; os alvos de `remove` e `purge` existem; o
/// filtro de `remove` acha pelo menos um evento anterior.
pub fn check_against(
    log: &SpecLog,
    event: &Map<String, Value>,
    new_id: u64,
) -> Result<Effects, Refusal> {
    let event_type = event.get("type").and_then(Value::as_str).unwrap_or_default();
    if let Some(old) = event.get("replaces").and_then(Value::as_u64) {
        let Some(previous) = log.get(old) else {
            return Err(Refusal::UnknownTarget { target: EventRef::Id(old) });
        };
        if previous.event_type != event_type {
            return Err(Refusal::ReplacesOtherType {
                id: old,
                found: previous.event_type.clone(),
                event_type: event_type.to_string(),
            });
        }
    }
    let mut targets = ints(event.get("targets"));
    if let Some(unknown) = targets.iter().find(|id| log.get(**id).is_none()) {
        return Err(Refusal::UnknownTarget { target: EventRef::Id(*unknown) });
    }
    let mut effects = Effects::default();
    match event_type {
        "remove" => {
            if let Some(filter) = event.get("filter").and_then(TimeFilter::from_value) {
                let matched = log.filter_matches(&filter, new_id);
                if matched.is_empty() {
                    return Err(Refusal::FilterMatchesNothing {
                        event_type: filter.event_type,
                        from: filter.from,
                        to: filter.to,
                    });
                }
                targets.extend(matched);
            }
            targets.sort_unstable();
            targets.dedup();
            effects.removed = targets;
        }
        "purge" => {
            targets.sort_unstable();
            targets.dedup();
            effects.purged = targets;
        }
        _ => {}
    }
    Ok(effects)
}

/// O evento pronto para o arquivo: a versão do formato, o número, o código do
/// item (veja [`code_after`]), a hora e o campo de busca, calculado de `text`
/// e `keys`.
#[must_use]
pub fn stamp(mut event: Map<String, Value>, id: u64, code: Option<&str>, at: &str) -> Map<String, Value> {
    event.insert("v".into(), Value::from(FORMAT_VERSION));
    event.insert("id".into(), Value::from(id));
    if let Some(code) = code {
        event.insert("code".into(), Value::String(code.to_string()));
    }
    event.insert("at".into(), Value::String(at.to_string()));
    if let Some(search) = search_of(&event) {
        event.insert("search".into(), Value::String(search));
    }
    event
}

/// A linha do arquivo: o envelope na ordem fixa, os outros campos em ordem
/// alfabética e o `search` por último. A ordem não depende de como o JSON foi
/// lido, então a mesma entrada dá sempre os mesmos bytes.
#[must_use]
pub fn render_line(event: &Map<String, Value>) -> String {
    render(event, true)
}

/// A linha como a leitura mostra: igual à do arquivo, sem o `search`, que
/// nunca é mostrado.
#[must_use]
pub fn shown_line(event: &Map<String, Value>) -> String {
    render(event, false)
}

fn render(event: &Map<String, Value>, with_search: bool) -> String {
    let mut keys: Vec<&str> = LEAD_FIELDS.iter().copied().filter(|k| event.contains_key(*k)).collect();
    let mut rest: Vec<&str> = event
        .keys()
        .map(String::as_str)
        .filter(|k| !LEAD_FIELDS.contains(k) && *k != "search")
        .collect();
    rest.sort_unstable();
    keys.extend(rest);
    if with_search && event.contains_key("search") {
        keys.push("search");
    }
    let mut out = String::from("{");
    for (i, key) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_str(&mut out, key);
        out.push(':');
        write_value(&mut out, &event[*key]);
    }
    out.push('}');
    out
}

fn write_str(out: &mut String, s: &str) {
    out.push_str(&Value::String(s.to_string()).to_string());
}

/// JSON compacto com as chaves de cada objeto em ordem alfabética.
fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(out, key);
                out.push(':');
                write_value(out, &map[key.as_str()]);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// O arquivo depois do expurgo: cada linha de `targets` fica só com o
/// envelope e com a marca do expurgo; as outras linhas, inclusive as que não
/// se entendem, ficam byte a byte como estavam.
#[must_use]
pub fn purge_lines(content: &str, targets: &[u64], by: u64) -> String {
    content
        .split('\n')
        .map(|raw| {
            let line = raw.trim_end_matches('\r');
            match parse_object(line) {
                Some(map) if map.get("id").and_then(Value::as_u64).is_some_and(|id| targets.contains(&id)) => {
                    render_line(&purged_envelope(&map, by))
                }
                _ => raw.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// O que sobra de uma linha expurgada: o envelope, a origem, o `replaces` e a
/// marca com o número do expurgo. Todo texto sai.
fn purged_envelope(map: &Map<String, Value>, by: u64) -> Map<String, Value> {
    let mut kept = Map::new();
    for key in LEAD_FIELDS.iter().chain(["origin", "replaces"].iter()) {
        if let Some(value) = map.get(*key) {
            kept.insert((*key).to_string(), value.clone());
        }
    }
    kept.insert(PURGED_FIELD.into(), Value::from(by));
    kept
}

/// O arquivo com o `search` de cada linha recalculado pelo redutor de hoje,
/// e quantas linhas mudaram. Só é reescrita a linha que tem texto ou chaves e
/// cujo `search` faltava ou era outro; as outras, inclusive as que não se
/// entendem, ficam byte a byte como estavam. É o que o índice das specs roda
/// quando o redutor muda.
#[must_use]
pub fn refresh_search_lines(content: &str) -> (String, usize) {
    let mut changed = 0usize;
    let body = content
        .split('\n')
        .map(|raw| {
            let line = raw.trim_end_matches('\r');
            let Some(mut map) = parse_object(line) else { return raw.to_string() };
            let Some(search) = search_of(&map) else { return raw.to_string() };
            if map.get("search").and_then(Value::as_str) == Some(search.as_str()) {
                return raw.to_string();
            }
            map.insert("search".into(), Value::String(search));
            changed += 1;
            render_line(&map)
        })
        .collect::<Vec<_>>()
        .join("\n");
    (body, changed)
}

// ---------------------------------------------------------------------------
// Busca
// ---------------------------------------------------------------------------

/// As raízes das palavras de um texto: minúsculas, cada palavra reduzida à
/// raiz pelo redutor de português e, depois, sem acento. "apagar",
/// "apagando" e "apagou" dão a mesma raiz. Sem repetição, na ordem em que
/// aparecem.
fn roots<'a>(pieces: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let stemmer = Stemmer::create(Algorithm::Portuguese);
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for piece in pieces {
        let lower = piece.to_lowercase();
        for word in text::words(&lower) {
            let root = text::fold_accents(&stemmer.stem(word));
            if seen.insert(root.clone()) {
                out.push(root);
            }
        }
    }
    out
}

/// O campo `search`: as raízes de `text` e de `keys`, separadas por espaço.
/// Calculado só pelo binário e nunca mostrado.
#[must_use]
pub fn search_field(text: Option<&str>, keys: &[&str]) -> String {
    roots(text.into_iter().chain(keys.iter().copied())).join(" ")
}

/// As raízes de um termo de busca, para comparar com o `search` de cada
/// evento.
#[must_use]
pub fn search_terms(query: &str) -> Vec<String> {
    roots([query])
}

/// O `search` que uma linha deve ter, calculado de `text` e de `keys`.
/// `None` para a linha sem texto e sem chaves, que fica sem o campo.
fn search_of(event: &Map<String, Value>) -> Option<String> {
    let text = event.get("text").and_then(Value::as_str);
    let keys: Vec<&str> = event
        .get("keys")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    (text.is_some() || !keys.is_empty()).then(|| search_field(text, &keys))
}

// ---------------------------------------------------------------------------
// Leitura
// ---------------------------------------------------------------------------

/// Um evento lido do arquivo.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecEvent {
    pub id: u64,
    pub event_type: String,
    /// O número da linha no arquivo, a partir de 1.
    pub line: usize,
    /// A linha inteira, como foi lida.
    pub fields: Map<String, Value>,
}

impl SpecEvent {
    /// A data e a hora do evento.
    #[must_use]
    pub fn at(&self) -> &str {
        self.str_field("at").unwrap_or_default()
    }

    /// Um campo de texto.
    #[must_use]
    pub fn str_field(&self, field: &str) -> Option<&str> {
        self.fields.get(field).and_then(Value::as_str)
    }

    /// Um campo de número.
    #[must_use]
    pub fn int(&self, field: &str) -> Option<u64> {
        self.fields.get(field).and_then(Value::as_u64)
    }

    /// Um campo de lista de números; vazio quando falta.
    #[must_use]
    pub fn ints(&self, field: &str) -> Vec<u64> {
        ints(self.fields.get(field))
    }

    /// O bloco do tipo; `None` para um tipo que este binário não conhece.
    #[must_use]
    pub fn block(&self) -> Option<Block> {
        type_spec(&self.event_type).map(|t| t.block)
    }

    /// O número da onda a que o evento pertence: `n` na onda, `wave` na
    /// tarefa, no envio, no entregou e no veredito.
    #[must_use]
    pub fn wave(&self) -> Option<u64> {
        match self.event_type.as_str() {
            "wave" => self.int("n"),
            "task" | "send" | "delivered" | "verdict" => self.int("wave"),
            _ => None,
        }
    }

    /// `true` quando o evento tem todas as raízes do termo, somando as do
    /// `search` e as do código do item (`code`, o que a leitura dá a ele): o
    /// `search` não guarda o código, e sem ele um item não seria achado pelo
    /// próprio código que a página mostra. `None` olha só o `search`.
    #[must_use]
    pub fn matches(&self, terms: &[String], code: Option<&str>) -> bool {
        if terms.is_empty() {
            return true;
        }
        let code_roots = code.map(|c| roots([c])).unwrap_or_default();
        let mut words: BTreeSet<&str> = self.str_field("search").unwrap_or_default().split(' ').collect();
        words.extend(code_roots.iter().map(String::as_str));
        terms.iter().all(|t| words.contains(t.as_str()))
    }

    /// A linha como a leitura mostra, sem o `search`.
    #[must_use]
    pub fn shown(&self) -> String {
        shown_line(&self.fields)
    }
}

fn ints(value: Option<&Value>) -> Vec<u64> {
    value
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

/// Por que uma linha foi pulada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// A linha não é um objeto JSON com `id` e `type`: a máquina desligou no
    /// meio de uma gravação, ou alguém editou o arquivo à mão.
    Unreadable,
    /// A linha repete um número que outra linha acima já usa.
    DuplicateId(u64),
}

/// Uma linha que a leitura pulou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkippedLine {
    /// O número da linha no arquivo, a partir de 1.
    pub line: usize,
    pub reason: SkipReason,
    /// O número que a linha parece ter, para que a próxima gravação nunca o
    /// repita.
    pub id_hint: Option<u64>,
}

impl SkippedLine {
    /// O aviso, no idioma pedido.
    #[must_use]
    pub fn message(&self, lang: Locale) -> String {
        match self.reason {
            SkipReason::Unreadable => {
                translate("spec_events.skipped_line", lang).replace("{line}", &self.line.to_string())
            }
            SkipReason::DuplicateId(id) => translate("spec_events.duplicate_id", lang)
                .replace("{line}", &self.line.to_string())
                .replace("{id}", &id.to_string()),
        }
    }
}

/// Por que um evento some da leitura.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hidden {
    /// Um `remove` o tirou; `by` é o número do `remove`.
    Removed { by: u64 },
    /// Uma versão nova, de número `by`, o substitui.
    Replaced { by: u64 },
    /// O expurgo de número `by` tirou o texto dele do arquivo.
    Purged { by: u64 },
}

/// Um filtro de remoção: um tipo e um intervalo de horário, na hora local de
/// quem lê. Os dois extremos entram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeFilter {
    pub event_type: String,
    pub from: String,
    pub to: String,
}

impl TimeFilter {
    /// O filtro de um campo `filter`; `None` quando falta alguma parte.
    #[must_use]
    pub fn from_value(value: &Value) -> Option<Self> {
        let get = |k: &str| value.get(k).and_then(Value::as_str).map(str::to_string);
        Some(Self { event_type: get("type")?, from: get("from")?, to: get("to")? })
    }

    /// `true` quando o evento é do tipo e o horário cai no intervalo. Compara
    /// só a parte local do `at`, até a precisão de cada extremo:
    /// `2026-09-11T21:10` pega tudo o que aconteceu nesse minuto.
    #[must_use]
    pub fn matches(&self, event: &SpecEvent) -> bool {
        if event.event_type != self.event_type {
            return false;
        }
        let local = event.at().get(..19).unwrap_or(event.at());
        let cut = |bound: &str| local.get(..bound.len().min(19)).unwrap_or(local).to_string();
        let from = self.from.get(..self.from.len().min(19)).unwrap_or(&self.from);
        let to = self.to.get(..self.to.len().min(19)).unwrap_or(&self.to);
        cut(from).as_str() >= from && cut(to).as_str() <= to
    }
}

/// Um passo do fluxo que lê a spec. Cada passo lê só os blocos dele.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Retomar: o estado.
    Resume,
    /// Despachar uma onda: o bloco da onda, os critérios dela, a
    /// especificação com os limites, os itens combinados que as tarefas dela
    /// cobrem e o entregou das ondas de que ela depende. Nunca a conversa.
    Dispatch { wave: u64 },
    /// Revisar uma onda: o bloco da onda, com o entregou dela, e os critérios
    /// dela.
    Review { wave: u64 },
    /// Fechar: o estado e os critérios.
    Close,
    /// Tirar uma dúvida: a conversa, filtrada pelo termo.
    Question { term: String },
}

/// O arquivo lido: os eventos em ordem e as linhas puladas.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpecLog {
    pub events: Vec<SpecEvent>,
    pub skipped: Vec<SkippedLine>,
}

/// Lê o conteúdo do arquivo. Nunca falha: a linha que não se entende é
/// pulada, com o número dela em [`SpecLog::skipped`], e o resto é lido. Uma
/// linha em branco não conta.
#[must_use]
pub fn parse_log(content: &str) -> SpecLog {
    let mut log = SpecLog::default();
    let mut seen = BTreeSet::new();
    for (i, raw) in content.split('\n').enumerate() {
        let line = i + 1;
        let raw = raw.trim_end_matches('\r');
        if raw.trim().is_empty() {
            continue;
        }
        let parsed = parse_object(raw).map(upgrade).and_then(|fields| {
            let id = fields.get("id").and_then(Value::as_u64).filter(|n| *n > 0)?;
            let event_type = fields.get("type").and_then(Value::as_str)?.to_string();
            Some(SpecEvent { id, event_type, line, fields })
        });
        match parsed {
            Some(event) if !seen.insert(event.id) => log.skipped.push(SkippedLine {
                line,
                reason: SkipReason::DuplicateId(event.id),
                id_hint: Some(event.id),
            }),
            Some(event) => log.events.push(event),
            None => log.skipped.push(SkippedLine {
                line,
                reason: SkipReason::Unreadable,
                id_hint: id_hint(raw),
            }),
        }
    }
    log
}

fn parse_object(line: &str) -> Option<Map<String, Value>> {
    match serde_json::from_str::<Value>(line).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// Traz uma linha de uma versão anterior do formato para a atual. A linha sem
/// `v` é da primeira versão. Quando o formato mudar, a conversão da versão
/// anterior entra aqui, e as specs antigas continuam sendo lidas; uma linha de
/// versão mais nova que este binário é lida como está.
fn upgrade(line: Map<String, Value>) -> Map<String, Value> {
    line
}

/// O número que uma linha estragada parece ter: o que vem depois de `"id":`.
fn id_hint(raw: &str) -> Option<u64> {
    let at = raw.find("\"id\"")?;
    let rest = raw[at + 4..].trim_start().strip_prefix(':')?.trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

impl SpecLog {
    /// O evento de número `id`, removido ou não.
    #[must_use]
    pub fn get(&self, id: u64) -> Option<&SpecEvent> {
        self.events.iter().find(|e| e.id == id)
    }

    /// O maior número do arquivo, contando o que as linhas estragadas parecem
    /// ter. A próxima gravação usa o seguinte, então um número nunca se
    /// repete, nem depois de uma edição à mão.
    #[must_use]
    pub fn max_id(&self) -> u64 {
        let read = self.events.iter().map(|e| e.id);
        let hinted = self.skipped.iter().filter_map(|s| s.id_hint);
        read.chain(hinted).max().unwrap_or(0)
    }

    /// Os eventos que um filtro de remoção pega, entre os gravados antes do
    /// número `before`.
    #[must_use]
    pub fn filter_matches(&self, filter: &TimeFilter, before: u64) -> Vec<u64> {
        self.events.iter().filter(|e| e.id < before && filter.matches(e)).map(|e| e.id).collect()
    }

    /// Os eventos que somem da leitura, cada um com o motivo. O expurgo vence;
    /// entre remoção e substituição, vale a primeira.
    #[must_use]
    pub fn hidden(&self) -> BTreeMap<u64, Hidden> {
        let mut hidden = BTreeMap::new();
        for event in &self.events {
            if let Some(by) = event.int(PURGED_FIELD) {
                hidden.insert(event.id, Hidden::Purged { by });
            }
            match event.event_type.as_str() {
                "purge" => {
                    for id in event.ints("targets") {
                        hidden.insert(id, Hidden::Purged { by: event.id });
                    }
                }
                "remove" => {
                    let mut targets = event.ints("targets");
                    if let Some(filter) = event.fields.get("filter").and_then(TimeFilter::from_value) {
                        targets.extend(self.filter_matches(&filter, event.id));
                    }
                    for id in targets {
                        hidden.entry(id).or_insert(Hidden::Removed { by: event.id });
                    }
                }
                _ => {}
            }
            if let Some(old) = event.int("replaces") {
                hidden.entry(old).or_insert(Hidden::Replaced { by: event.id });
            }
        }
        hidden
    }

    /// Os eventos que a leitura mostra, em ordem.
    #[must_use]
    pub fn visible(&self) -> Vec<&SpecEvent> {
        let hidden = self.hidden();
        self.events.iter().filter(|e| !hidden.contains_key(&e.id)).collect()
    }

    /// A versão vigente de um item: segue as substituições a partir de `id` e
    /// devolve a última, se ela não foi removida.
    #[must_use]
    pub fn current(&self, id: u64) -> Option<&SpecEvent> {
        let replaced_by: BTreeMap<u64, u64> =
            self.events.iter().filter_map(|e| e.int("replaces").map(|old| (old, e.id))).collect();
        let mut id = id;
        for _ in 0..=self.events.len() {
            match replaced_by.get(&id) {
                Some(next) => id = *next,
                None => break,
            }
        }
        let hidden = self.hidden();
        self.get(id).filter(|e| !hidden.contains_key(&e.id))
    }

    /// Um bloco, só com o que a leitura mostra. Uma onda (`wave-2`) traz a
    /// onda, as tarefas, os envios e os entregou dela, e as skills que as
    /// tarefas dela nomeiam.
    #[must_use]
    pub fn block(&self, query: BlockQuery) -> Vec<&SpecEvent> {
        let visible = self.visible();
        match query {
            BlockQuery::Block(Block::Metrics) => visible
                .into_iter()
                .filter(|e| METRIC_TYPES.contains(&e.event_type.as_str()))
                .collect(),
            BlockQuery::Block(block) => visible.into_iter().filter(|e| e.block() == Some(block)).collect(),
            BlockQuery::Wave(n) => {
                let skills: BTreeSet<&str> = visible
                    .iter()
                    .filter(|e| e.event_type == "task" && e.wave() == Some(n))
                    .filter_map(|e| e.str_field("skill"))
                    .collect();
                visible
                    .iter()
                    .copied()
                    .filter(|e| e.block() == Some(Block::Waves))
                    .filter(|e| {
                        e.wave() == Some(n)
                            || (e.event_type == "skill"
                                && e.str_field("name").is_some_and(|s| skills.contains(s)))
                    })
                    .collect()
            }
        }
    }

    /// O que um passo do fluxo lê, em ordem de número, sem repetição.
    #[must_use]
    pub fn step(&self, step: &Step) -> Vec<&SpecEvent> {
        let mut picked: BTreeMap<u64, &SpecEvent> = BTreeMap::new();
        match step {
            Step::Resume => pick(&mut picked,self.block(BlockQuery::Block(Block::State))),
            Step::Close => {
                pick(&mut picked,self.block(BlockQuery::Block(Block::State)));
                pick(&mut picked,self.block(BlockQuery::Block(Block::Criteria)));
            }
            Step::Question { term } => {
                let terms = search_terms(term);
                let codes = self.codes();
                pick(&mut picked,self
                    .block(BlockQuery::Block(Block::Conversation))
                    .into_iter()
                    .filter(|e| e.matches(&terms, codes.get(&e.id).map(String::as_str)))
                    .collect());
            }
            Step::Review { wave } => {
                pick(&mut picked,self.block(BlockQuery::Wave(*wave)));
                pick(&mut picked,self.wave_criteria(*wave));
            }
            Step::Dispatch { wave } => {
                let own = self.block(BlockQuery::Wave(*wave));
                let covered: Vec<&SpecEvent> = own
                    .iter()
                    .filter(|e| e.event_type == "task")
                    .flat_map(|e| e.ints("covers"))
                    .filter_map(|id| self.current(id))
                    .collect();
                let depends: Vec<u64> = own
                    .iter()
                    .filter(|e| e.event_type == "wave")
                    .flat_map(|e| e.ints("depends_on"))
                    .collect();
                let delivered: Vec<&SpecEvent> = depends
                    .into_iter()
                    .flat_map(|d| self.block(BlockQuery::Wave(d)))
                    .filter(|e| e.event_type == "delivered")
                    .collect();
                let limits: Vec<&SpecEvent> = self
                    .block(BlockQuery::Block(Block::Agreed))
                    .into_iter()
                    .filter(|e| e.event_type == "limit")
                    .collect();
                pick(&mut picked,own);
                pick(&mut picked,self.wave_criteria(*wave));
                pick(&mut picked,self.block(BlockQuery::Block(Block::Specification)));
                pick(&mut picked,limits);
                pick(&mut picked,covered);
                pick(&mut picked,delivered);
            }
        }
        picked.into_values().collect()
    }

}

/// Junta eventos por número, sem repetição.
fn pick<'a>(picked: &mut BTreeMap<u64, &'a SpecEvent>, events: Vec<&'a SpecEvent>) {
    for event in events {
        picked.insert(event.id, event);
    }
}

impl SpecLog {
    /// Os critérios que a onda `n` aponta, na versão vigente de cada um.
    fn wave_criteria(&self, n: u64) -> Vec<&SpecEvent> {
        self.block(BlockQuery::Wave(n))
            .into_iter()
            .filter(|e| e.event_type == "wave")
            .flat_map(|e| e.ints("criteria"))
            .filter_map(|id| self.current(id))
            .collect()
    }

    /// O código de cada evento, `MSTD-<sigla>-<NNNN>`, pelo número do evento.
    ///
    /// O código gravado na linha vale como está. Uma linha sem código, de uma
    /// edição à mão ou de um gravador anterior, recebe o da versão que ela
    /// substitui (`replaces`) ou, senão, o próximo número livre do tipo: o
    /// seguinte ao maior visto até ela no arquivo, pulando os que outra linha
    /// já gravou. Assim os códigos gravados nunca mudam, e o de uma linha sem
    /// código não muda quando outra linha é gravada no fim. Um tipo que este
    /// binário não conhece fica sem código. Cada spec conta do zero.
    #[must_use]
    pub fn codes(&self) -> BTreeMap<u64, String> {
        let mut codes: BTreeMap<u64, String> = BTreeMap::new();
        let mut taken: BTreeSet<(&str, u64)> = BTreeSet::new();
        for event in &self.events {
            if let Some((kind, n)) = recorded_code(event) {
                codes.insert(event.id, mustard_id::format(kind, n));
                taken.insert((kind, n));
            }
        }
        let mut highest: BTreeMap<&str, u64> = BTreeMap::new();
        for event in &self.events {
            let Some(spec) = type_spec(&event.event_type) else {
                continue;
            };
            let top = highest.entry(spec.code).or_insert(0);
            if let Some((_, n)) = recorded_code(event) {
                *top = (*top).max(n);
                continue;
            }
            let inherited = event
                .int("replaces")
                .filter(|old| self.get(*old).is_some_and(|o| o.event_type == event.event_type))
                .and_then(|old| codes.get(&old).cloned());
            if let Some(code) = inherited {
                codes.insert(event.id, code);
                continue;
            }
            let mut n = *top + 1;
            while taken.contains(&(spec.code, n)) {
                n += 1;
            }
            *top = n;
            codes.insert(event.id, mustard_id::format(spec.code, n));
        }
        codes
    }
}

/// O código gravado numa linha, como sigla e número, quando ele tem o formato
/// e a sigla do tipo da linha. Um código de outro tipo ou fora do formato
/// conta como ausente.
fn recorded_code(event: &SpecEvent) -> Option<(&'static str, u64)> {
    let spec = type_spec(&event.event_type)?;
    let (kind, n) = mustard_id::parse(event.str_field("code")?.trim())?;
    (kind == spec.code).then_some((spec.code, n))
}

/// O código que o evento `event` grava ao entrar no fim de `log`: o da versão
/// que ele substitui, quando `replaces` aponta um item do mesmo tipo; senão, o
/// maior número que o tipo já tem na spec, mais 1. Nunca a posição: um número
/// que saiu do meio do arquivo não volta. `None` para um tipo desconhecido.
#[must_use]
pub fn code_after(log: &SpecLog, event: &Map<String, Value>) -> Option<String> {
    let spec = type_spec(event.get("type").and_then(Value::as_str)?)?;
    let codes = log.codes();
    let inherited = event
        .get("replaces")
        .and_then(Value::as_u64)
        .filter(|old| log.get(*old).is_some_and(|o| o.event_type == spec.name))
        .and_then(|old| codes.get(&old).cloned());
    if inherited.is_some() {
        return inherited;
    }
    let top = codes
        .values()
        .filter_map(|code| mustard_id::parse(code))
        .filter(|(kind, _)| *kind == spec.code)
        .map(|(_, n)| n)
        .max()
        .unwrap_or(0);
    Some(mustard_id::format(spec.code, top + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    fn checked(event_type: &str, draft: Value) -> Result<(), Refusal> {
        validate(&normalize(obj(draft), event_type))
    }

    #[test]
    fn there_are_thirty_three_types_each_with_one_block() {
        assert_eq!(TYPES.len(), 33);
        let names: BTreeSet<&str> = TYPES.iter().map(|t| t.name).collect();
        assert_eq!(names.len(), 33, "a type name repeats");
        for block in Block::ALL {
            if block == Block::Metrics {
                assert!(TYPES.iter().all(|t| t.block != block), "nobody writes to the panel");
            } else {
                assert!(TYPES.iter().any(|t| t.block == block), "{} has no type", block.name());
            }
        }
        for name in METRIC_TYPES {
            assert!(type_spec(name).is_some(), "the panel reads an unknown type {name}");
        }
    }

    #[test]
    fn every_block_name_reads_back_and_a_wave_takes_its_number() {
        for block in Block::ALL {
            assert_eq!(BlockQuery::parse(block.name()), Some(BlockQuery::Block(block)));
        }
        assert_eq!(BlockQuery::parse("wave-2"), Some(BlockQuery::Wave(2)));
        assert_eq!(BlockQuery::parse("wave-x"), None);
        assert_eq!(BlockQuery::parse("everything"), None);
        assert!(BlockQuery::accepted_names().contains("waves, wave-<n>, review"));
    }

    #[test]
    fn an_unknown_type_is_refused_by_name() {
        let refusal = checked("licao", json!({"text": "x"})).unwrap_err();
        assert_eq!(refusal, Refusal::UnknownType { found: "licao".into() });
        assert!(refusal.message(Locale::PtBr).contains("O tipo licao não existe"));
        assert!(refusal.message(Locale::EnUs).contains("no licao event type"));
    }

    #[test]
    fn a_missing_or_blank_required_field_is_refused_by_name() {
        let base = json!({"text": "Só o binário grava.", "keys": ["gravação"], "origin": 3});
        assert_eq!(
            checked("rule", base.clone()).unwrap_err(),
            Refusal::MissingField { event_type: "rule".into(), field: "example".into() }
        );
        let mut blank = base;
        blank["example"] = json!("   ");
        assert_eq!(
            checked("rule", blank).unwrap_err(),
            Refusal::MissingField { event_type: "rule".into(), field: "example".into() }
        );
        assert_eq!(
            checked("note", json!({"text": "t", "keys": [], "origin": 1})).unwrap_err(),
            Refusal::MissingField { event_type: "note".into(), field: "keys".into() }
        );
    }

    #[test]
    fn what_the_assistant_writes_from_the_conversation_needs_its_origin() {
        assert_eq!(
            checked("note", json!({"text": "t", "keys": ["k"]})).unwrap_err(),
            Refusal::MissingField { event_type: "note".into(), field: "origin".into() }
        );
        assert!(checked("message", json!({"text": "oi", "author": "user"})).is_ok());
    }

    /// Uma tarefa que não cita arquivo entra sem o campo; quando o campo vem,
    /// cada arquivo continua precisando do caminho.
    #[test]
    fn a_task_without_files_is_accepted_and_a_file_without_path_is_not() {
        assert!(checked("task", json!({"wave": 1, "text": "Medir de novo.", "origin": 1})).is_ok());
        assert_eq!(
            checked("task", json!({"wave": 1, "text": "t", "files": [{"new": true}], "origin": 1})).unwrap_err(),
            Refusal::MissingField { event_type: "task".into(), field: "files[1].path".into() }
        );
    }

    #[test]
    fn a_value_of_the_wrong_shape_is_refused_with_the_expected_shape() {
        let refusal =
            checked("hook", json!({"hook": "g", "action": "stop", "tool": "Bash", "reason": "r"}))
                .unwrap_err();
        assert_eq!(
            refusal,
            Refusal::InvalidValue {
                event_type: "hook".into(),
                field: "action".into(),
                expected: Kind::OneOf(HOOK_ACTIONS)
            }
        );
        assert!(refusal.message(Locale::PtBr).contains("uma destas palavras: warn, block"));
        let author = checked("message", json!({"text": "oi", "author": "robot"})).unwrap_err();
        assert!(matches!(author, Refusal::InvalidValue { ref field, .. } if field == "author"));
    }

    #[test]
    fn a_fact_without_source_is_refused_by_its_position() {
        let point = json!({
            "block": "limits", "gap": "tamanho", "from": "gap", "status": "open", "origin": 1,
            "facts": [{"text": "a", "source": "src/a.rs:1"}, {"text": "b"}]
        });
        assert_eq!(checked("point", point).unwrap_err(), Refusal::FactWithoutSource { fact: 2 });
    }

    #[test]
    fn situations_that_require_a_field() {
        let approved = json!({"phase": "approved", "author": "binary"});
        assert_eq!(
            checked("state", approved).unwrap_err(),
            Refusal::MissingField { event_type: "state".into(), field: "witness".into() }
        );
        let witness = json!({"phase": "approved", "author": "binary", "witness": {"question": "Aprovar?"}});
        assert_eq!(
            checked("state", witness).unwrap_err(),
            Refusal::MissingField { event_type: "state".into(), field: "witness.answer".into() }
        );
        let born = json!({"name": "s", "action": "create", "text": "t", "sha": "1",
            "examples": [{"path": "a.rs", "why": "w"}]});
        assert!(matches!(
            checked("skill", born).unwrap_err(),
            Refusal::WrongCount { min: 2, max: 3, count: 1, .. }
        ));
        let nothing = json!({"reason": "engano"});
        assert_eq!(
            checked("remove", nothing).unwrap_err(),
            Refusal::MissingField { event_type: "remove".into(), field: "targets".into() }
        );
        let bad_time = json!({"reason": "r", "filter": {"type": "message", "from": "21:03", "to": "21:10"}});
        assert!(matches!(
            checked("remove", bad_time).unwrap_err(),
            Refusal::InvalidValue { ref field, .. } if field == "filter.from"
        ));
    }

    #[test]
    fn delete_and_its_inflections_share_one_search_root() {
        let a = search_terms("apagar");
        assert_eq!(a, search_terms("apagando"));
        assert_eq!(a, search_terms("apagou"));
        let field = search_field(Some("Conciliação da ação"), &["Pagamento"]);
        assert!(!field.contains('ç') && !field.contains('ã'), "{field}");
        assert!(field.split(' ').any(|w| w == search_terms("pagamentos")[0]), "{field}");
    }

    #[test]
    fn a_line_starts_with_the_envelope_and_ends_with_search() {
        let event = stamp(normalize(obj(json!({"text": "x", "keys": ["k"], "origin": 1})), "note"), 7, None, "t");
        let line = render_line(&event);
        assert!(line.starts_with(r#"{"v":1,"id":7,"at":"t","type":"note","author":"assistant","keys":"#), "{line}");
        assert!(line.ends_with(r#""search":"x k"}"#), "{line}");
        assert!(!shown_line(&event).contains("search"));
    }

    /// Recalcular o `search` reescreve só a linha em que ele faltava ou era
    /// outro; a linha certa, a que não se entende e a que não tem texto ficam
    /// byte a byte.
    #[test]
    fn refreshing_the_search_rewrites_only_the_stale_lines() {
        let right = render_line(&stamp(
            normalize(obj(json!({"text": "Apagar a pasta.", "keys": ["pasta"], "origin": 1})), "note"),
            1,
            None,
            "t",
        ));
        let stale = r#"{"v":1,"id":2,"at":"t","type":"note","author":"assistant","keys":["k"],"text":"Trava nova.","search":"velho"}"#;
        let older = r#"{"id":3,"type":"rule","text":"Sem busca gravada."}"#;
        let bare = r#"{"v":1,"id":4,"at":"t","type":"message","author":"user","purged":9}"#;
        let content = format!("{right}\n{stale}\ngarbage\r\n{older}\n{bare}\n");

        let (fixed, changed) = refresh_search_lines(&content);
        assert_eq!(changed, 2, "{fixed}");
        let lines: Vec<&str> = fixed.split('\n').collect();
        assert_eq!(lines[0], right);
        assert_eq!(lines[2], "garbage\r", "a line that does not parse stays as it was");
        assert_eq!(lines[4], bare);
        assert_eq!(lines[5], "", "the file still ends with a newline");
        let log = parse_log(&fixed);
        assert_eq!(log.get(2).unwrap().str_field("search"), Some(search_field(Some("Trava nova."), &["k"]).as_str()));
        assert_eq!(log.get(3).unwrap().str_field("search"), Some(search_field(Some("Sem busca gravada."), &[]).as_str()));
        assert_eq!(refresh_search_lines(&fixed), (fixed.clone(), 0), "a second pass changes nothing");
    }

    #[test]
    fn file_citations_are_told_apart_from_other_sources() {
        assert_eq!(file_citation("src/a.rs:10"), Some(("src/a.rs".into(), 10)));
        assert_eq!(file_citation("src\\a.rs:3-9"), Some(("src/a.rs".into(), 9)));
        assert_eq!(file_citation("cargo test → 3 passed"), None);
        assert_eq!(file_citation("354"), None);
        assert_eq!(file_citation("https://example.com:8080"), None);
        assert_eq!(file_citation("README:10"), None);
    }

    #[test]
    fn a_broken_line_is_skipped_by_number_and_the_rest_is_read() {
        let content = "{\"v\":1,\"id\":1,\"at\":\"t\",\"type\":\"message\",\"text\":\"a\"}\n\
                       garbage\n\
                       {\"v\":1,\"id\":1,\"at\":\"t\",\"type\":\"note\"}\n\
                       {\"v\":1,\"id\":2,\"at\":\"t\",\"type\":\"message\",\"text\":\"b\"}\n\
                       {\"v\":1,\"id\":9,\"at\":\"t\",\"ty";
        let log = parse_log(content);
        assert_eq!(log.events.iter().map(|e| e.id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(
            log.skipped.iter().map(|s| (s.line, s.reason)).collect::<Vec<_>>(),
            [(2, SkipReason::Unreadable), (3, SkipReason::DuplicateId(1)), (5, SkipReason::Unreadable)]
        );
        assert_eq!(log.max_id(), 9, "the torn line's number is never reused");
        assert!(log.skipped[0].message(Locale::PtBr).contains("linha 2"));
    }

    #[test]
    fn lines_the_writer_would_refuse_are_still_read() {
        let content = "{\"id\":1,\"type\":\"rule\",\"text\":\"sem versão e sem keys\"}\n\
                       {\"v\":7,\"id\":2,\"type\":\"future_kind\",\"shape\":\"new\"}\n";
        let log = parse_log(content);
        assert_eq!(log.events.len(), 2);
        assert!(log.skipped.is_empty());
        assert_eq!(log.events[1].block(), None, "an unknown type belongs to no block");
    }

    /// Cada tipo tem uma sigla própria, só de letras maiúsculas, e o código
    /// montado com ela tem o formato do identificador do Mustard.
    #[test]
    fn every_type_has_its_own_code_letters() {
        let codes: BTreeSet<&str> = TYPES.iter().map(|t| t.code).collect();
        assert_eq!(codes.len(), TYPES.len(), "two types share a code");
        for t in TYPES {
            assert!(!t.code.is_empty() && t.code.bytes().all(|b| b.is_ascii_uppercase()), "{}", t.name);
            assert!(mustard_id::is_id(&mustard_id::format(t.code, 1)), "{}", t.name);
        }
    }

    fn line(id: u64, event_type: &str, extra: &str) -> String {
        format!("{{\"v\":1,\"id\":{id},\"at\":\"2026-09-12T10:00:00-03:00\",\"type\":\"{event_type}\"{extra}}}\n")
    }

    /// Os códigos contam por tipo, na ordem do arquivo; a versão nova herda o
    /// código da antiga; um item removido ou expurgado segue contando, e o
    /// número dele nunca é dado de novo.
    #[test]
    fn codes_count_per_type_and_a_number_never_returns() {
        let content = [
            line(1, "rule", ",\"text\":\"a\""),
            line(2, "criterion", ",\"when\":\"w\""),
            line(3, "rule", ",\"text\":\"b\""),
            line(4, "remove", ",\"targets\":[3],\"reason\":\"x\""),
            line(5, "rule", ",\"text\":\"a2\",\"replaces\":1"),
            line(6, "purge", ",\"targets\":[2],\"reason\":\"secret\""),
            line(7, "rule", ",\"text\":\"c\""),
            line(8, "criterion", ",\"when\":\"w2\""),
            line(9, "future_kind", ""),
        ]
        .concat();
        let log = parse_log(&content);
        let codes = log.codes();
        assert_eq!(codes[&1], "MSTD-RULE-0001");
        assert_eq!(codes[&2], "MSTD-CRIT-0001");
        assert_eq!(codes[&3], "MSTD-RULE-0002");
        assert_eq!(codes[&4], "MSTD-RMV-0001");
        assert_eq!(codes[&5], "MSTD-RULE-0001", "the new version is the same item");
        assert_eq!(codes[&7], "MSTD-RULE-0003", "the removed number 2 never returns");
        assert_eq!(codes[&8], "MSTD-CRIT-0002", "the purged number 1 never returns");
        assert!(!codes.contains_key(&9), "an unknown type has no code");

        // O código do próximo evento sai do mesmo cálculo.
        let next = obj(json!({"id": 10, "type": "rule", "text": "d"}));
        assert_eq!(code_after(&log, &next).as_deref(), Some("MSTD-RULE-0004"));
        let revised = obj(json!({"id": 10, "type": "rule", "text": "c2", "replaces": 7}));
        assert_eq!(code_after(&log, &revised).as_deref(), Some("MSTD-RULE-0003"));

        // Outra spec conta do zero.
        let other = parse_log(&line(1, "rule", ",\"text\":\"z\""));
        assert_eq!(other.codes()[&1], "MSTD-RULE-0001");
    }

    /// O código gravado na linha vale como está. Uma linha sem código recebe
    /// o próximo número livre do tipo, pulando os que outra linha gravou, e
    /// esse número não muda quando outra linha é gravada no fim. Um código de
    /// outra sigla não vale.
    #[test]
    fn recorded_codes_stand_and_a_line_without_one_takes_the_next_free_number() {
        let coded = |id: u64, event_type: &str, code: &str, extra: &str| {
            line(id, event_type, &format!(",\"code\":\"{code}\"{extra}"))
        };
        let mut content = [
            coded(1, "rule", "MSTD-RULE-0001", ""),
            line(2, "rule", ",\"text\":\"inserida à mão\""),
            coded(3, "rule", "MSTD-RULE-0002", ""),
            coded(4, "rule", "MSTD-RULE-0003", ""),
            coded(5, "criterion", "MSTD-CRIT-0007", ""),
            line(6, "rule", ",\"text\":\"revista à mão\",\"replaces\":3"),
            coded(7, "rule", "MSTD-CRIT-0009", ""),
        ]
        .concat();
        let codes = parse_log(&content).codes();
        let got: Vec<&str> = (1..=7).map(|id| codes[&id].as_str()).collect();
        assert_eq!(
            got,
            [
                "MSTD-RULE-0001",
                "MSTD-RULE-0004",
                "MSTD-RULE-0002",
                "MSTD-RULE-0003",
                "MSTD-CRIT-0007",
                "MSTD-RULE-0002",
                "MSTD-RULE-0005",
            ]
        );
        let next = obj(json!({"type": "rule", "text": "nova"}));
        assert_eq!(code_after(&parse_log(&content), &next).as_deref(), Some("MSTD-RULE-0006"));
        assert_eq!(
            code_after(&parse_log(&content), &obj(json!({"type": "criterion"}))).as_deref(),
            Some("MSTD-CRIT-0008"),
            "the next number follows the largest, never the position"
        );

        content.push_str(&coded(8, "rule", "MSTD-RULE-0006", ""));
        let after = parse_log(&content).codes();
        assert_eq!((after[&2].as_str(), after[&7].as_str()), ("MSTD-RULE-0004", "MSTD-RULE-0005"));
    }

    /// O código não vem de quem grava: o evento que o traz é recusado, com a
    /// mensagem nos dois idiomas. Os outros campos do binário continuam
    /// descartados.
    #[test]
    fn a_code_sent_by_the_caller_is_refused() {
        let draft = json!({"text": "t", "keys": ["k"], "origin": 1, "code": "MSTD-NOTE-0001"});
        let refusal = checked("note", draft).unwrap_err();
        assert_eq!(refusal, Refusal::BinaryOnlyField { field: "code".into() });
        let (pt, en) = (refusal.message(Locale::PtBr), refusal.message(Locale::EnUs));
        assert!(pt.contains("O campo code é gravado só pelo binário"), "{pt}");
        assert!(en.contains("The code field is written only by the binary"), "{en}");
        assert!(checked("note", json!({"text": "t", "keys": ["k"], "origin": 1, "id": 9, "at": "x"})).is_ok());
    }

    /// Um código aponta o item: em `replaces`, a versão mais nova; nos alvos,
    /// todas as versões. Um código que não existe é recusado citando o código,
    /// nos dois idiomas, e um texto fora do formato nem passa da conferência.
    #[test]
    fn a_code_points_at_its_item_and_an_unknown_code_is_refused() {
        let log = parse_log(
            &[
                line(1, "rule", ",\"code\":\"MSTD-RULE-0001\""),
                line(2, "rule", ",\"code\":\"MSTD-RULE-0002\""),
                line(3, "rule", ",\"code\":\"MSTD-RULE-0002\",\"replaces\":2"),
            ]
            .concat(),
        );
        let mut removal = obj(json!({"type": "remove", "targets": ["MSTD-RULE-0002", 1, 3], "reason": "r"}));
        resolve_codes(&log, &mut removal).unwrap();
        assert_eq!(removal["targets"], json!([2, 3, 1]));
        let mut revision = obj(json!({"type": "rule", "replaces": "MSTD-RULE-0002"}));
        resolve_codes(&log, &mut revision).unwrap();
        assert_eq!(revision["replaces"], json!(3));

        let mut unknown = obj(json!({"type": "purge", "targets": ["MSTD-RULE-0009"], "reason": "secret"}));
        let refusal = resolve_codes(&log, &mut unknown).unwrap_err();
        assert_eq!(refusal, Refusal::UnknownTarget { target: EventRef::Code("MSTD-RULE-0009".into()) });
        assert_eq!(refusal.reason(), "unknown-target");
        assert!(refusal.message(Locale::PtBr).contains("O item MSTD-RULE-0009 não existe nesta spec"));
        assert!(refusal.message(Locale::EnUs).contains("Item MSTD-RULE-0009 does not exist in this spec"));

        assert!(checked("remove", json!({"targets": ["MSTD-RULE-0002"], "reason": "r"})).is_ok());
        assert!(matches!(
            checked("remove", json!({"targets": ["R2"], "reason": "r"})).unwrap_err(),
            Refusal::InvalidValue { ref field, expected: Kind::Refs, .. } if field == "targets"
        ));
    }
}
