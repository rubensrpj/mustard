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
//! Função pura: sem disco e sem relógio. A trava, a gravação e o caminho do
//! arquivo moram em `io::spec_events`.

use std::collections::{BTreeMap, BTreeSet};

use rust_stemmers::{Algorithm, Stemmer};
use serde_json::{Map, Value};

use crate::domain::text;
use crate::platform::i18n::{translate, Locale};

/// A versão do formato de cada linha. O leitor entende as anteriores.
pub const FORMAT_VERSION: u64 = 1;

/// Quem pode ter produzido um evento.
pub const AUTHORS: &[&str] = &["user", "assistant", "hook", "binary", "wave", "review", "skill"];

/// Quem grava pelo comando `write` sem dizer quem é: o assistente.
pub const DEFAULT_AUTHOR: &str = "assistant";

/// Os campos que só o binário escreve. O que vier neles de quem grava é
/// descartado e trocado.
pub const BINARY_FIELDS: &[&str] = &["v", "id", "at", "search"];

/// O campo que marca uma linha expurgada; guarda o número do expurgo.
pub const PURGED_FIELD: &str = "purged";

/// O envelope, na ordem em que abre cada linha do arquivo.
const LEAD_FIELDS: &[&str] = &["v", "id", "at", "type", "author"];

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
}

impl Kind {
    fn accepts(self, value: &Value) -> bool {
        let is_int = |v: &Value| v.is_i64() || v.is_u64();
        match self {
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

/// Um campo de um tipo.
#[derive(Debug, Clone, Copy)]
pub struct Field {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
}

const fn req(name: &'static str, kind: Kind) -> Field {
    Field { name, kind, required: true }
}

const fn opt(name: &'static str, kind: Kind) -> Field {
    Field { name, kind, required: false }
}

/// Um tipo de evento: o nome, o bloco, e os campos próprios.
#[derive(Debug)]
pub struct TypeSpec {
    pub name: &'static str,
    pub block: Block,
    /// O assistente grava este tipo a partir da conversa: o número da mensagem
    /// de onde ele veio (`origin`) é obrigatório.
    pub needs_origin: bool,
    pub fields: &'static [Field],
}

const fn ty(
    name: &'static str,
    block: Block,
    needs_origin: bool,
    fields: &'static [Field],
) -> TypeSpec {
    TypeSpec { name, block, needs_origin, fields }
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
    ty("message", Block::Conversation, false, &[TEXT]),
    ty("response", Block::Conversation, false, &[TEXT, req("reply_to", Kind::Int)]),
    ty(
        "injection",
        Block::Conversation,
        false,
        &[req("hook", Kind::Text), req("chars", Kind::Int), TEXT],
    ),
    ty(
        "hook",
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
    ty("work_type", Block::Agreed, true, &[req("kinds", Kind::ManyOf(WORK_KINDS))]),
    ty(
        "point",
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
    ty("rule", Block::Agreed, true, &[TEXT, KEYS, req("example", Kind::Text), APPLIES_TO]),
    ty("limit", Block::Agreed, true, &[TEXT, KEYS, req("value", Kind::Text), APPLIES_TO]),
    ty("contract", Block::Agreed, true, &[TEXT, KEYS, req("example", Kind::Text), APPLIES_TO]),
    ty("error", Block::Agreed, true, &[TEXT, KEYS, req("message", Kind::Text)]),
    ty("edge_case", Block::Agreed, true, &[TEXT, KEYS, req("expected", Kind::Text)]),
    ty("out_of_scope", Block::Agreed, true, &[TEXT, KEYS, opt("reason", Kind::Text)]),
    ty("decision", Block::Agreed, true, &[TEXT, KEYS, req("why", Kind::Text)]),
    // Especificação.
    ty("context", Block::Specification, true, &[TEXT]),
    ty("concern", Block::Specification, true, &[TEXT]),
    // Critérios.
    ty(
        "criterion",
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
        Block::Waves,
        true,
        &[
            req("wave", Kind::Int),
            TEXT,
            req("files", Kind::Objects),
            opt("skill", Kind::Text),
            opt("covers", Kind::Ints),
            opt("must_read", Kind::List),
        ],
    ),
    ty(
        "skill",
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
        Block::Waves,
        false,
        &[req("wave", Kind::Int), TEXT, req("files", Kind::Texts)],
    ),
    // Revisão.
    ty(
        "verdict",
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
    ty("pr_summary", Block::Progress, false, &[TEXT]),
    // Anotações.
    ty("request", Block::Notes, true, &[TEXT, KEYS, req("effect", Kind::OneOf(EFFECTS))]),
    ty("deferred", Block::Notes, true, &[TEXT, KEYS, req("pending", Kind::Int)]),
    ty("note", Block::Notes, true, &[TEXT, KEYS]),
    // Remoção e expurgo, na conversa.
    ty(
        "remove",
        Block::Conversation,
        false,
        &[req("reason", Kind::Text), opt("targets", Kind::Ints), opt("filter", Kind::Object)],
    ),
    ty(
        "purge",
        Block::Conversation,
        false,
        &[req("targets", Kind::Ints), req("reason", Kind::OneOf(PURGE_REASONS))],
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

/// Por que um evento não foi gravado ou um bloco não foi lido.
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
    UnknownTarget { id: u64 },
    ReplacesOtherType { id: u64, found: String, event_type: String },
    FilterMatchesNothing { event_type: String, from: String, to: String },
    UnknownBlock { found: String },
    BadSpecName { spec: String },
    NoSpecFile { spec: String },
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
            Self::UnknownTarget { .. } => "unknown-target",
            Self::ReplacesOtherType { .. } => "replaces-other-type",
            Self::FilterMatchesNothing { .. } => "filter-matches-nothing",
            Self::UnknownBlock { .. } => "unknown-block",
            Self::BadSpecName { .. } => "bad-spec-name",
            Self::NoSpecFile { .. } => "no-spec-file",
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
            Self::UnknownTarget { id } => {
                fill("spec_events.unknown_target", &[("{id}", id.to_string())])
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
            Self::Io { detail } => fill("spec_events.io_failed", &[("{detail}", detail.clone())]),
        }
    }
}

// ---------------------------------------------------------------------------
// Gravação: preparar, conferir, carimbar e escrever a linha
// ---------------------------------------------------------------------------

/// `true` para o que não vale como valor: ausente, `null`, texto em branco,
/// lista vazia ou objeto vazio. Número e `true`/`false` nunca são vazios.
fn is_empty(value: &Value) -> bool {
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
/// quem grava não diz).
#[must_use]
pub fn normalize(mut draft: Map<String, Value>, event_type: &str) -> Map<String, Value> {
    for field in BINARY_FIELDS {
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
    check_field(event, spec.name, req("author", Kind::OneOf(AUTHORS)))?;
    check_field(event, spec.name, Field { name: "origin", kind: Kind::Int, required: spec.needs_origin })?;
    for envelope in [opt("label", Kind::Text), opt("replaces", Kind::Int)] {
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

fn check_field(event: &Map<String, Value>, event_type: &str, field: Field) -> Result<(), Refusal> {
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
            return Err(Refusal::UnknownTarget { id: old });
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
        return Err(Refusal::UnknownTarget { id: *unknown });
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

/// O evento pronto para o arquivo: a versão do formato, o número, a hora e o
/// campo de busca, calculado de `text` e `keys`.
#[must_use]
pub fn stamp(mut event: Map<String, Value>, id: u64, at: &str) -> Map<String, Value> {
    event.insert("v".into(), Value::from(FORMAT_VERSION));
    event.insert("id".into(), Value::from(id));
    event.insert("at".into(), Value::String(at.to_string()));
    let text = event.get("text").and_then(Value::as_str);
    let keys: Vec<&str> = event
        .get("keys")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if text.is_some() || !keys.is_empty() {
        let search = search_field(text, &keys);
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

    /// `true` quando o `search` do evento tem todas as raízes do termo.
    #[must_use]
    pub fn matches(&self, terms: &[String]) -> bool {
        if terms.is_empty() {
            return true;
        }
        let words: BTreeSet<&str> = self.str_field("search").unwrap_or_default().split(' ').collect();
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
                pick(&mut picked,self
                    .block(BlockQuery::Block(Block::Conversation))
                    .into_iter()
                    .filter(|e| e.matches(&terms))
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
        let refusal = checked("lesson", json!({"text": "x"})).unwrap_err();
        assert_eq!(refusal, Refusal::UnknownType { found: "lesson".into() });
        assert!(refusal.message(Locale::PtBr).contains("O tipo lesson não existe"));
        assert!(refusal.message(Locale::EnUs).contains("no lesson event type"));
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
        let event = stamp(normalize(obj(json!({"text": "x", "keys": ["k"], "origin": 1})), "note"), 7, "t");
        let line = render_line(&event);
        assert!(line.starts_with(r#"{"v":1,"id":7,"at":"t","type":"note","author":"assistant","keys":"#), "{line}");
        assert!(line.ends_with(r#""search":"x k"}"#), "{line}");
        assert!(!shown_line(&event).contains("search"));
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
}
