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

use crate::domain::spec_state::original_of;
use crate::domain::survey::{self, GapKey};
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
/// O item combinado que não vira código: o valor é o motivo. Quem o traz sai
/// do aviso dos itens sem tarefa, porque não há tarefa que o implemente.
const NO_CODE: Field = opt("no_code", Kind::Text);

const HOOK_ACTIONS: &[&str] = &["warn", "block"];
const CALL_RESULTS: &[&str] = &["ok", "refused"];
/// O teto de caracteres do texto de um entregou.
pub const DELIVERED_MAX_CHARS: usize = 8_000;

/// As fases de uma spec, na ordem em que acontecem.
pub const PHASES: &[&str] =
    &["survey", "plan", "approved", "running", "closed", "pr_open", "delivered", "discarded"];
const PAGES: &[&str] = &["spec"];
const MILESTONES: &[&str] = &["approval", "round", "close"];
/// Os tipos de trabalho, na ordem em que o levantamento junta as lacunas.
pub const WORK_KINDS: &[&str] = &["feature", "fix", "refactor"];
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
    // Conversa. A mensagem que responde a um gesto de aprovação leva a
    // testemunha: a pergunta e a opção que o usuário clicou.
    ty("message", "MSG", Block::Conversation, false, &[TEXT, opt("witness", Kind::Object)]),
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
            opt("closes", Kind::Ref),
            opt("result", Kind::Ints),
            opt("reason", Kind::Text),
            opt("reminders", Kind::List),
        ],
    ),
    ty("rule", "RULE", Block::Agreed, true, &[TEXT, KEYS, req("example", Kind::Text), APPLIES_TO, NO_CODE]),
    ty("limit", "LIMIT", Block::Agreed, true, &[TEXT, KEYS, req("value", Kind::Text), APPLIES_TO, NO_CODE]),
    ty("contract", "CONTR", Block::Agreed, true, &[TEXT, KEYS, req("example", Kind::Text), APPLIES_TO, NO_CODE]),
    ty("error", "ERR", Block::Agreed, true, &[TEXT, KEYS, req("message", Kind::Text), NO_CODE]),
    ty("edge_case", "EDGE", Block::Agreed, true, &[TEXT, KEYS, req("expected", Kind::Text), NO_CODE]),
    ty("out_of_scope", "SCOPE", Block::Agreed, true, &[TEXT, KEYS, opt("reason", Kind::Text), NO_CODE]),
    ty("decision", "DEC", Block::Agreed, true, &[TEXT, KEYS, req("why", Kind::Text), NO_CODE]),
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
            // A relação dos itens da onda em ordem de execução. É ela que o
            // pedido leva, uma linha por item; sem ela, os itens saem na
            // ordem do arquivo.
            opt("order", Kind::Ints),
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
            // O pedido exato, como foi injetado no agente. É por ele que se
            // confere depois se a onda recebeu o que devia.
            TEXT,
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
    ("message", "witness", &["question", "answer"]),
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
    /// Um campo que o tipo não declara. Ele entraria calado e ficaria gravado
    /// sem ninguém ver, e nunca seria lido por nada.
    UnknownField { event_type: String, field: String, accepted: String },
    UnknownTarget { target: EventRef },
    ReplacesOtherType { id: u64, found: String, event_type: String },
    FilterMatchesNothing { event_type: String, from: String, to: String },
    UnknownBlock { found: String },
    BadSpecName { spec: String },
    NoSpecFile { spec: String },
    /// Uma gravação numa spec que ninguém abriu: sem arquivo de eventos e sem
    /// estado. O arquivo nasce com a branch, no comando que abre a spec.
    SpecNotOpen { spec: String },
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
    /// O `run write` com o autor `binary`, que fica só para as gravações de
    /// dentro do binário.
    BinaryAuthor,
    /// O `run write` com uma mensagem do usuário, ou uma gravação dele que
    /// tiraria ou reveria uma: a fala do usuário chega pelos ganchos.
    UserMessageByHook { spec: String },
    /// Uma gravação, ou uma página, numa pasta de spec do formato antigo: o
    /// `spec.md` dela é o documento, e o binário não grava nela.
    OldFormatSpec { spec: String },
    /// O pedido adiado (`deferred`) aponta uma pendência que a lista não tem.
    DeferredUnknownPending { pending: String },
    /// O pedido adiado aponta uma pendência já fechada ou descartada.
    DeferredClosedPending { pending: String },
    /// O primeiro `context` de uma spec em levantamento, o objetivo, não
    /// aponta uma mensagem do usuário ou não repete o texto dela.
    GoalNotVerbatim { spec: String, origin: String },
    /// O `run write` com o tipo `work_type`, ou uma gravação dele que tiraria
    /// ou reveria o tipo de trabalho: quem o grava é o `grill`.
    WorkTypeByGrill,
    /// A passagem para o plano, ou a aprovação, de uma spec com pontos do
    /// levantamento abertos: quantos e quais, com o código, o número e a
    /// lacuna de cada um.
    SurveyOpen { spec: String, count: usize, points: String },
    /// A passagem para o plano de uma spec cujo levantamento não gravou o
    /// tipo de trabalho.
    SurveyNotStarted { spec: String },
    /// A passagem para o plano de uma spec com lacunas do tipo de trabalho
    /// ainda sem ponto; a mensagem as nomeia no idioma de quem lê.
    SurveyGapsUnrecorded { spec: String, gaps: Vec<GapKey> },
    /// Um `point` novo no mesmo bloco e com a mesma lacuna de um ponto que já
    /// está aberto: o que existe volta no lugar do segundo.
    PointAlreadyOpen { code: String, block: String },
    /// Um `point` que fecha (`closes`) o que não é um ponto aberto; a lista
    /// dos abertos vai junto.
    PointNotOpen { id: String, open: String },
    /// Um `point` que fecha outro, mas continua com a situação `open`.
    ClosingPointOpen,
    /// Um `point` marcado "não se aplica" sem o motivo.
    NotApplicableNeedsReason,
    /// Um `remove` que tiraria um ponto aberto da leitura.
    OpenPointRemoved { code: String },
    /// Um `purge` que apagaria o texto de um ponto aberto.
    OpenPointPurged { code: String },
    /// Um `remove` ou um `purge` que tiraria o ponto que fecha outro cujo
    /// original já saiu: ele é o único registro do ponto.
    ClosingPointLastRecord { code: String },
    /// O pedido montado de uma onda passa do teto de linhas mesmo com o
    /// combinado reduzido a ponteiros: a onda precisa ser dividida antes de o
    /// plano ir para a aprovação, e `parts` diz o que ficou inteiro nela,
    /// cada parte com quantas linhas ocupa.
    WavePromptTooLong { wave: u64, lines: usize, max: usize, parts: String },
    /// O texto do entregou de uma onda passa do teto de caracteres: ele volta
    /// para a janela principal e precisa caber nela.
    DeliveredTooLong { chars: usize, max: usize },
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
            Self::UnknownField { .. } => "unknown-field",
            Self::UnknownTarget { .. } => "unknown-target",
            Self::ReplacesOtherType { .. } => "replaces-other-type",
            Self::FilterMatchesNothing { .. } => "filter-matches-nothing",
            Self::UnknownBlock { .. } => "unknown-block",
            Self::BadSpecName { .. } => "bad-spec-name",
            Self::NoSpecFile { .. } => "no-spec-file",
            Self::SpecNotOpen { .. } => "spec-not-open",
            Self::NoCurrentSpec => "no-current-spec",
            Self::SpecRequired { .. } => "spec-required",
            Self::UnknownLesson { .. } => "unknown-lesson",
            Self::LessonOriginMissing => "lesson-origin-missing",
            Self::PhaseChangeRefused { .. } => "phase-change-refused",
            Self::StateByFlowOnly { .. } => "state-by-flow-only",
            Self::BinaryOnlyType { .. } => "binary-only-type",
            Self::BinaryAuthor => "binary-author",
            Self::UserMessageByHook { .. } => "user-message-by-hook",
            Self::OldFormatSpec { .. } => "old-format-spec",
            Self::DeferredUnknownPending { .. } => "deferred-unknown-pending",
            Self::DeferredClosedPending { .. } => "deferred-closed-pending",
            Self::GoalNotVerbatim { .. } => "goal-not-verbatim",
            Self::WorkTypeByGrill => "work-type-by-grill",
            Self::SurveyOpen { .. } => "survey-open",
            Self::SurveyNotStarted { .. } => "survey-not-started",
            Self::SurveyGapsUnrecorded { .. } => "survey-gaps-unrecorded",
            Self::PointAlreadyOpen { .. } => "point-already-open",
            Self::PointNotOpen { .. } => "point-not-open",
            Self::ClosingPointOpen => "closing-point-open",
            Self::NotApplicableNeedsReason => "not-applicable-needs-reason",
            Self::OpenPointRemoved { .. } => "open-point-removed",
            Self::OpenPointPurged { .. } => "open-point-purged",
            Self::ClosingPointLastRecord { .. } => "closing-point-last-record",
            Self::WavePromptTooLong { .. } => "wave-prompt-too-long",
            Self::DeliveredTooLong { .. } => "delivered-too-long",
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
            Self::UnknownField { event_type, field, accepted } => fill(
                "spec_events.unknown_field",
                &[
                    ("{type}", event_type.clone()),
                    ("{field}", field.clone()),
                    ("{fields}", accepted.clone()),
                ],
            ),
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
            Self::SpecNotOpen { spec } => {
                fill("spec_events.spec_not_open", &[("{spec}", spec.clone())])
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
            Self::OldFormatSpec { spec } => {
                fill("spec_events.old_format_spec", &[("{spec}", spec.clone())])
            }
            Self::StateByFlowOnly { spec } => {
                fill("spec_events.state_by_flow_only", &[("{spec}", spec.clone())])
            }
            Self::BinaryOnlyType { event_type, spec } => fill(
                "spec_events.binary_only_type",
                &[("{type}", event_type.clone()), ("{spec}", spec.clone())],
            ),
            Self::BinaryAuthor => fill("spec_events.binary_author", &[]),
            Self::UserMessageByHook { spec } => {
                fill("spec_events.user_message_by_hook", &[("{spec}", spec.clone())])
            }
            Self::DeferredUnknownPending { pending } => {
                fill("spec_events.deferred_unknown_pending", &[("{pending}", pending.clone())])
            }
            Self::DeferredClosedPending { pending } => {
                fill("spec_events.deferred_closed_pending", &[("{pending}", pending.clone())])
            }
            Self::GoalNotVerbatim { spec, origin } => fill(
                "spec_events.goal_not_verbatim",
                &[("{spec}", spec.clone()), ("{origin}", origin.clone())],
            ),
            Self::WorkTypeByGrill => fill("grill.work_type_by_grill", &[]),
            Self::SurveyOpen { spec, count, points } => fill(
                "spec_events.survey_open",
                &[("{spec}", spec.clone()), ("{count}", count.to_string()), ("{points}", points.clone())],
            ),
            Self::SurveyNotStarted { spec } => fill("spec_events.survey_not_started", &[("{spec}", spec.clone())]),
            Self::SurveyGapsUnrecorded { spec, gaps } => fill(
                "spec_events.survey_gaps_unrecorded",
                &[
                    ("{spec}", spec.clone()),
                    ("{count}", gaps.len().to_string()),
                    ("{gaps}", gaps.iter().map(|gap| gap.label(lang)).collect::<Vec<_>>().join("; ")),
                ],
            ),
            Self::PointAlreadyOpen { code, block } => fill(
                "spec_events.point_already_open",
                &[("{code}", code.clone()), ("{block}", block.clone())],
            ),
            Self::PointNotOpen { id, open } => {
                fill("spec_events.point_not_open", &[("{id}", id.clone()), ("{open}", open.clone())])
            }
            Self::ClosingPointOpen => fill("spec_events.closing_point_open", &[]),
            Self::NotApplicableNeedsReason => fill("spec_events.not_applicable_reason", &[]),
            Self::OpenPointRemoved { code } => fill("spec_events.open_point_removed", &[("{code}", code.clone())]),
            Self::OpenPointPurged { code } => fill("spec_events.open_point_purged", &[("{code}", code.clone())]),
            Self::ClosingPointLastRecord { code } => {
                fill("spec_events.closing_point_last_record", &[("{code}", code.clone())])
            }
            Self::WavePromptTooLong { wave, lines, max, parts } => fill(
                "spec_events.wave_prompt_too_long",
                &[
                    ("{wave}", wave.to_string()),
                    ("{lines}", lines.to_string()),
                    ("{max}", max.to_string()),
                    ("{parts}", parts.clone()),
                ],
            ),
            Self::DeliveredTooLong { chars, max } => fill(
                "spec_events.delivered_too_long",
                &[("{chars}", chars.to_string()), ("{max}", max.to_string())],
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
    // O `origin` é obrigatório no que o assistente grava a partir da
    // conversa; o que o binário grava, como os critérios tirados do
    // `spec.md`, não tem mensagem de onde veio.
    let by_assistant = event.get("author").and_then(Value::as_str) == Some(DEFAULT_AUTHOR);
    check_field(
        event,
        spec.name,
        Field { name: "origin", kind: Kind::Int, required: spec.needs_origin && by_assistant },
    )?;
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
    // O campo que o tipo não declara é recusado pelo nome: ele entraria
    // calado e ficaria gravado sem ninguém ver, e um nome escrito errado
    // nunca mais seria lido por nada.
    if let Some(found) = event.keys().find(|key| !accepts_field(spec, key)) {
        return Err(Refusal::UnknownField {
            event_type: spec.name.to_string(),
            field: found.clone(),
            accepted: accepted_fields(spec),
        });
    }
    check_nested(event, spec.name)?;
    check_conditions(event, spec.name)?;
    check_fact_sources(event, spec.name)
}

/// Os campos que toda linha pode trazer, fora os do tipo: o envelope, o campo
/// de busca, a marca do expurgo, o rótulo, a versão nova de um item e a
/// mensagem de onde ele veio.
const COMMON_FIELDS: &[&str] =
    &["v", "id", "code", "at", "type", "author", "search", "purged", "label", "replaces", "origin", "text", "keys"];

/// O tipo aceita este campo? Aceita os comuns a toda linha e os que ele
/// declara.
fn accepts_field(spec: &TypeSpec, name: &str) -> bool {
    COMMON_FIELDS.contains(&name) || spec.fields.iter().any(|field| field.name == name)
}

/// Os campos que um tipo aceita, separados por vírgula: os que ele declara,
/// depois os comuns a toda linha.
fn accepted_fields(spec: &TypeSpec) -> String {
    let own: Vec<&str> = spec.fields.iter().map(|field| field.name).collect();
    let rest: Vec<&str> = COMMON_FIELDS.iter().copied().filter(|name| !own.contains(name)).collect();
    own.into_iter().chain(rest).collect::<Vec<_>>().join(", ")
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
/// remoção. O ponto que fecha outro (`closes`) nunca fica aberto, e o que
/// "não se aplica" leva o motivo.
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
            let status = word("status");
            if status == "open" {
                if has("closes") {
                    return Err(Refusal::ClosingPointOpen);
                }
                return need("facts");
            }
            // A versão nova de um ponto pode vir sem `closes`: o binário copia
            // o da versão antiga e confere de novo (veja
            // [`carry_closed_identity`]).
            if !has("replaces") {
                need("closes")?;
            }
            if status == "not_applicable" {
                return if has("reason") { Ok(()) } else { Err(Refusal::NotApplicableNeedsReason) };
            }
            if has("result") || has("reason") { Ok(()) } else { Err(missing(event_type, "result")) }
        }
        // O entregou volta para a janela principal a cada onda: ele conta o
        // que mudou, e não repete o pedido.
        "delivered" => {
            let chars = word("text").chars().count();
            if chars > DELIVERED_MAX_CHARS {
                return Err(Refusal::DeliveredTooLong { chars, max: DELIVERED_MAX_CHARS });
            }
            Ok(())
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

/// A leitura da fonte que cita um arquivo mora na conferência das citações.
pub use crate::domain::citation::file_citation;

/// Troca cada código (`MSTD-RULE-0002`) dos campos que apontam eventos pelos
/// números que ele nomeia no arquivo como está, para que a linha gravada
/// guarde só números: em `replaces` e no `closes` de um ponto, a versão mais
/// nova do item; nos alvos de `remove` e `purge`, todas as versões dele. Um
/// código que não existe na spec recusa o evento. Um evento sem código nenhum
/// sai como entrou.
pub fn resolve_codes(log: &SpecLog, event: &mut Map<String, Value>) -> Result<(), Refusal> {
    let is_code = |v: &Value| matches!(EventRef::from_value(v), Some(EventRef::Code(_)));
    let replaces_code = event.get("replaces").is_some_and(is_code);
    let closes_code = event.get("closes").is_some_and(is_code);
    let targets = event.get("targets").and_then(Value::as_array).cloned().unwrap_or_default();
    if !replaces_code && !closes_code && !targets.iter().any(is_code) {
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
    for field in ["replaces", "closes"] {
        if let Some(EventRef::Code(code)) = event.get(field).and_then(EventRef::from_value) {
            let newest = ids_of(&code)?.last().copied().unwrap_or_default();
            event.insert(field.into(), Value::from(newest));
        }
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
///
/// No ponto do levantamento: o `closes` aponta um ponto aberto, por qualquer
/// versão dele (a versão nova de um fechamento continua fechando o mesmo
/// ponto), e cada número de `result` existe. Um ponto aberto não sai com
/// `remove` nem com `purge`, por nenhuma versão: ele só fecha, com a resposta
/// ou o motivo, e o texto dele só é apagado depois de fechado. O fechamento
/// de um ponto cujo original já saiu, ou sai junto, também não sai: é o único
/// registro do ponto.
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
    // De onde o evento veio é um evento que já está no arquivo: o número que
    // não existe, e o número do próprio evento, não dizem origem nenhuma.
    if let Some(origin) = event.get("origin").and_then(Value::as_u64)
        && (origin == new_id || log.get(origin).is_none())
    {
        return Err(Refusal::UnknownTarget { target: EventRef::Id(origin) });
    }
    let mut targets = ints(event.get("targets"));
    if let Some(unknown) = targets.iter().find(|id| log.get(**id).is_none()) {
        return Err(Refusal::UnknownTarget { target: EventRef::Id(*unknown) });
    }
    let mut effects = Effects::default();
    match event_type {
        "point" => {
            if let Some(unknown) = ints(event.get("result")).into_iter().find(|id| log.get(*id).is_none()) {
                return Err(Refusal::UnknownTarget { target: EventRef::Id(unknown) });
            }
            if let Some(target) = event.get("closes").and_then(Value::as_u64) {
                check_closes(log, event, target)?;
            } else if event.get("replaces").is_none()
                && let Some(existing) = same_open_point(log, event)
            {
                return Err(Refusal::PointAlreadyOpen {
                    code: log.codes().get(&existing.id).cloned().unwrap_or_else(|| existing.id.to_string()),
                    block: existing.str_field("block").unwrap_or_default().trim().to_string(),
                });
            }
        }
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
            if let Some(code) = open_point_among(log, &targets) {
                return Err(Refusal::OpenPointRemoved { code });
            }
            if let Some(code) = last_record_among(log, &targets) {
                return Err(Refusal::ClosingPointLastRecord { code });
            }
            effects.removed = targets;
        }
        "purge" => {
            targets.sort_unstable();
            targets.dedup();
            if let Some(code) = open_point_among(log, &targets) {
                return Err(Refusal::OpenPointPurged { code });
            }
            if let Some(code) = last_record_among(log, &targets) {
                return Err(Refusal::ClosingPointLastRecord { code });
            }
            effects.purged = targets;
        }
        _ => {}
    }
    Ok(effects)
}

/// O ponto aberto que já está no arquivo com o mesmo bloco e a mesma lacuna
/// do evento `event`: gravar o segundo deixaria dois pontos abertos pedindo a
/// mesma resposta, e o levantamento apresentaria a mesma pergunta duas vezes.
/// Um evento sem bloco ou sem lacuna não repete ponto nenhum.
fn same_open_point<'a>(log: &'a SpecLog, event: &Map<String, Value>) -> Option<&'a SpecEvent> {
    let text = |name: &str| event.get(name).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let (block, gap) = (text("block")?, text("gap")?);
    survey::open_points(log).into_iter().find(|point| {
        point.str_field("block").map(str::trim) == Some(block) && point.str_field("gap").map(str::trim) == Some(gap)
    })
}

/// O `closes` de um ponto aponta um ponto aberto, por qualquer versão dele. A
/// versão nova de um fechamento, que aponta o mesmo ponto que o fechamento
/// revisto, também passa. Senão, a recusa traz o código, o número e a lacuna
/// dos pontos abertos, para o próximo fechamento acertar.
fn check_closes(log: &SpecLog, event: &Map<String, Value>, target: u64) -> Result<(), Refusal> {
    let first_version = |id: u64| log.get(id).filter(|e| e.event_type == "point").map(|e| original_of(log, e));
    let open = survey::open_points(log);
    let wanted = first_version(target);
    if let Some(wanted) = wanted {
        if open.iter().any(|p| original_of(log, p) == wanted) {
            return Ok(());
        }
        let revised = event
            .get("replaces")
            .and_then(Value::as_u64)
            .and_then(|id| log.get(id))
            .and_then(|old| old.int("closes"))
            .and_then(first_version);
        if revised == Some(wanted) {
            return Ok(());
        }
    }
    let id = log.codes().get(&target).map_or_else(|| target.to_string(), |code| format!("{code} ({target})"));
    Err(Refusal::PointNotOpen { id, open: survey::describe(log, &open) })
}

/// O código do primeiro ponto aberto entre os alvos de um `remove` ou de um
/// `purge`, por qualquer versão dele; `None` quando nenhum alvo é ponto
/// aberto.
fn open_point_among(log: &SpecLog, targets: &[u64]) -> Option<String> {
    let open: BTreeSet<u64> = survey::open_points(log).into_iter().map(|p| original_of(log, p)).collect();
    let point = targets
        .iter()
        .filter_map(|id| log.get(*id))
        .find(|e| e.event_type == "point" && open.contains(&original_of(log, e)))?;
    Some(log.codes().get(&point.id).cloned().unwrap_or_else(|| point.id.to_string()))
}

/// O código do primeiro fechamento, entre os alvos de um `remove` ou de um
/// `purge`, que é o único registro do ponto que fecha: o original dele já
/// saiu, ou sai junto. A versão velha de um fechamento revisto pode sair,
/// porque a nova fica; `None` quando nenhum alvo é um desses fechamentos.
fn last_record_among(log: &SpecLog, targets: &[u64]) -> Option<String> {
    let leaves = |event: &SpecEvent| targets.contains(&event.id);
    let closing = survey::points(log).into_iter().find_map(|point| {
        let closing = point.closing().filter(|closing| leaves(closing))?;
        point.original().is_none_or(leaves).then_some(closing)
    })?;
    Some(log.codes().get(&closing.id).cloned().unwrap_or_else(|| closing.id.to_string()))
}

/// O ponto que fecha outro carrega a identidade dele. A versão nova
/// (`replaces`) de um fechamento recebe o mesmo `closes` da versão antiga,
/// qualquer que seja o que veio no pedido, e por isso nunca tira o ponto da
/// leitura. Depois, a lacuna (`gap`) e a origem (`from`) do ponto fechado,
/// lidas pelo par, entram no fechamento, qualquer que seja a lacuna que veio
/// no pedido: a lacuna segue coberta pelo fechamento depois que o original
/// sai. Outro evento sai como entrou.
///
/// Com o `closes` no lugar, o ponto é conferido de novo: a versão que tenta
/// reabrir o ponto é recusada como todo ponto aberto que fecha outro, e o
/// ponto que não está aberto e segue sem `closes` é recusado pela falta dele.
pub fn carry_closed_identity(log: &SpecLog, event: &mut Map<String, Value>) -> Result<(), Refusal> {
    if event.get("type").and_then(Value::as_str) != Some("point") {
        return Ok(());
    }
    let inherited = event
        .get("replaces")
        .and_then(Value::as_u64)
        .and_then(|id| log.get(id))
        .filter(|old| old.event_type == "point")
        .and_then(|old| old.int("closes"));
    if let Some(closes) = inherited {
        event.insert("closes".into(), Value::from(closes));
    }
    let open = event.get("status").and_then(Value::as_str) == Some("open");
    match (open, event.get("closes").is_some_and(|v| !is_empty(v))) {
        (true, true) => return Err(Refusal::ClosingPointOpen),
        (false, false) => return Err(missing("point", "closes")),
        _ => {}
    }
    let Some(target) = event.get("closes").and_then(Value::as_u64).and_then(|id| log.get(id)) else {
        return Ok(());
    };
    if target.event_type != "point" {
        return Ok(());
    }
    let first = original_of(log, target);
    let Some(point) = survey::points(log).into_iter().find(|point| point.first() == first) else {
        return Ok(());
    };
    for field in ["gap", "from"] {
        if let Some(value) = point.shown().fields.get(field) {
            event.insert(field.to_string(), value.clone());
        }
    }
    Ok(())
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

/// Os eventos que um termo acha, do mais forte para o menos forte.
///
/// O termo que é o código de um item devolve exatamente esse item: é assim que
/// a conversa e a página citam um item, e um código nunca é uma busca por
/// assunto. Qualquer outro termo passa pela busca por nota que o projeto já
/// usa nas lições e no recorte dos itens por onda, sobre o campo de busca de
/// cada evento: quem casa mais forte vem primeiro, e não é preciso ter todas
/// as palavras do termo. O termo vazio devolve tudo, na ordem do arquivo.
#[must_use]
pub fn found_by<'a>(
    events: Vec<&'a SpecEvent>,
    term: &str,
    codes: &BTreeMap<u64, String>,
) -> Vec<&'a SpecEvent> {
    let term = term.trim();
    if term.is_empty() {
        return events;
    }
    let by_code: BTreeSet<u64> = codes
        .iter()
        .filter(|(_, code)| code.eq_ignore_ascii_case(term))
        .map(|(id, _)| *id)
        .collect();
    if !by_code.is_empty() {
        return events.into_iter().filter(|e| by_code.contains(&e.id)).collect();
    }
    let docs = events.iter().map(|e| (e.id, e.str_field("search").unwrap_or_default()));
    let ranked = crate::domain::search::SearchIndex::build(docs)
        .top(&crate::domain::search::query_terms(term), events.len());
    let by_id: BTreeMap<u64, &SpecEvent> = events.iter().map(|e| (e.id, *e)).collect();
    ranked.into_iter().filter_map(|hit| by_id.get(&hit.id).copied()).collect()
}

/// O número da onda a que uma linha pertence: `n` na onda, `wave` na tarefa,
/// no envio, no entregou e no veredito. `None` nos outros tipos.
fn wave_of(event_type: &str, field: impl Fn(&str) -> Option<u64>) -> Option<u64> {
    match event_type {
        "wave" => field("n"),
        "task" | "send" | "delivered" | "verdict" => field("wave"),
        _ => None,
    }
}

/// Os caminhos dos arquivos que a linha cita: cada item de `files`, que vem
/// como objeto com o campo do caminho ou já como o caminho em texto.
fn cited_paths(event: &Map<String, Value>) -> Vec<&str> {
    event
        .get("files")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|item| item.as_str().or_else(|| item.get("path").and_then(Value::as_str)))
                .collect()
        })
        .unwrap_or_default()
}

/// O `search` que uma linha deve ter: as raízes do texto e das palavras-chave,
/// mais o rótulo do item, o nome do tipo em palavras nos dois idiomas, a onda
/// a que a linha pertence e os caminhos dos arquivos que ela cita. Com isso,
/// procurar pelo nome de um arquivo acha as tarefas que mexem nele, e procurar
/// por "onda 13" acha o que é dela. `None` para a linha que não tem nada
/// disso, que fica sem o campo.
fn search_of(event: &Map<String, Value>) -> Option<String> {
    let text = event.get("text").and_then(Value::as_str);
    let mut extra: Vec<String> = Vec::new();
    if let Some(keys) = event.get("keys").and_then(Value::as_array) {
        extra.extend(keys.iter().filter_map(Value::as_str).map(str::to_string));
    }
    // A linha sem texto e sem palavras-chave — a expurgada, entre outras —
    // fica sem o campo; o resto só enriquece quem já tem o que procurar.
    if text.is_none() && extra.is_empty() {
        return None;
    }
    if let Some(label) = event.get("label").and_then(Value::as_str) {
        extra.push(label.to_string());
    }
    let event_type = event.get("type").and_then(Value::as_str).unwrap_or_default();
    if type_spec(event_type).is_some() {
        let key = format!("page.type.{event_type}");
        extra.push(translate(&key, Locale::PtBr).to_string());
        extra.push(translate(&key, Locale::EnUs).to_string());
    }
    if let Some(n) = wave_of(event_type, |f| event.get(f).and_then(Value::as_u64)) {
        extra.push(format!(
            "{} {n} {} {n}",
            translate("page.type.wave", Locale::PtBr),
            translate("page.type.wave", Locale::EnUs)
        ));
    }
    extra.extend(cited_paths(event).into_iter().map(str::to_string));
    let keys: Vec<&str> = extra.iter().map(String::as_str).collect();
    Some(search_field(text, &keys))
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
        wave_of(&self.event_type, |field| self.int(field))
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
    /// especificação com os limites, os itens combinados que o binário
    /// escolhe para ela e o entregou das ondas de que ela depende. Nunca a
    /// conversa.
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

    /// As ondas que já têm registro de entrega. O que elas fizeram está
    /// provado pelo código que entrou, e não pelo texto que o descreveu.
    #[must_use]
    pub fn delivered_waves(&self) -> BTreeSet<u64> {
        self.block(BlockQuery::Block(Block::Waves))
            .into_iter()
            .filter(|event| event.event_type == "delivered")
            .filter_map(SpecEvent::wave)
            .collect()
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
                let codes = self.codes();
                pick(
                    &mut picked,
                    found_by(self.block(BlockQuery::Block(Block::Conversation)), term, &codes),
                );
            }
            Step::Review { wave } => {
                pick(&mut picked,self.block(BlockQuery::Wave(*wave)));
                pick(&mut picked,self.wave_criteria(*wave));
            }
            Step::Dispatch { wave } => {
                let own = self.block(BlockQuery::Wave(*wave));
                let covered = crate::domain::wave_prompt::agreed_for(self, *wave);
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


// ---------------------------------------------------------------------------
// A mensagem do pull request — montada daqui, nunca escrita à mão
// ---------------------------------------------------------------------------

/// O teto de caracteres do título de um pull request e de um commit.
pub const MESSAGE_TITLE_MAX: usize = 60;
/// O teto de caracteres do corpo de um pull request e de um commit.
pub const MESSAGE_BODY_MAX: usize = 4_000;

/// O que uma mensagem de commit ou de pull request nunca leva, com o texto que
/// a recusa mostra. A busca é feita sobre o texto dobrado
/// ([`crate::domain::text::fold`]), por isso cada agulha vem em minúscula e sem
/// acento.
///
/// O caminho da máquina entra aqui pelas duas grafias que ele tem: um pull
/// request que cita `/home/alguem/projetos` diz o nome de quem trabalha e a
/// árvore de pastas dessa pessoa, que é dado de usuário como qualquer outro.
const FORBIDDEN_IN_MESSAGE: &[(&str, &str)] = &[
    ("claude.ai", "claude.ai"),
    ("claude", "Claude"),
    ("anthropic", "Anthropic"),
    ("co-authored-by", "Co-Authored-By"),
    ("generated with", "Generated with"),
    ("/home/", "/home/"),
    ("c:\\users\\", "C:\\Users\\"),
];

/// Por que uma mensagem de commit ou de pull request foi recusada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRefusal {
    /// O título ou o corpo passou do teto.
    TooLong {
        /// `title` ou `body`.
        part: &'static str,
        /// Quantos caracteres a parte tem.
        chars: usize,
        /// Quantos ela podia ter.
        max: usize,
    },
    /// A mensagem traz o que ela nunca leva. `found` é o trecho pelo nome e
    /// `excerpt` é o pedaço da mensagem em que ele apareceu, para que a recusa
    /// aponte onde está em vez de mandar procurar.
    Forbidden {
        /// O trecho proibido, pelo nome.
        found: String,
        /// O pedaço da mensagem em que ele apareceu.
        excerpt: String,
    },
    /// A spec não tem objetivo escrito, e é dele que sai o título.
    NoTitle,
}

impl MessageRefusal {
    /// O motivo estável, para quem lê a resposta como dado.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::TooLong { .. } => "message-too-long",
            Self::Forbidden { .. } => "message-forbidden-text",
            Self::NoTitle => "message-no-title",
        }
    }

    /// A recusa em palavras, no idioma pedido.
    #[must_use]
    pub fn message(&self, lang: Locale) -> String {
        match self {
            Self::TooLong { part, chars, max } => translate("message.too_long", lang)
                .replace("{part}", part)
                .replace("{chars}", &chars.to_string())
                .replace("{max}", &max.to_string()),
            Self::Forbidden { found, excerpt } => translate("message.forbidden", lang)
                .replace("{found}", found)
                .replace("{excerpt}", excerpt),
            Self::NoTitle => translate("message.no_title", lang).to_string(),
        }
    }
}

/// O pedaço de `text` em volta de `at`, com no máximo 40 caracteres de cada
/// lado: é o que a recusa mostra para que ninguém precise procurar.
fn excerpt_around(text: &str, at: usize) -> String {
    let start = text[..at].char_indices().rev().take(40).last().map_or(0, |(i, _)| i);
    let end = text[at..]
        .char_indices()
        .take(40)
        .last()
        .map_or(text.len(), |(i, c)| at + i + c.len_utf8());
    text[start..end].trim().to_string()
}

/// O primeiro e-mail que o texto traz: uma palavra, um arroba e um domínio com
/// ponto.
fn first_email(text: &str) -> Option<(String, usize)> {
    for (at, _) in text.match_indices('@') {
        let start = text[..at].rfind(char::is_whitespace).map_or(0, |i| i + 1);
        let end = text[at..].find(char::is_whitespace).map_or(text.len(), |i| at + i);
        let candidate = text[start..end].trim_matches(|c: char| !c.is_alphanumeric());
        let Some((user, domain)) = candidate.split_once('@') else { continue };
        if !user.is_empty() && domain.contains('.') && !domain.starts_with('.') {
            return Some((candidate.to_string(), start));
        }
    }
    None
}

/// Confere uma mensagem de commit ou de pull request contra o modelo: título e
/// corpo dentro do teto, e nada do que ela nunca leva.
///
/// A conferência é uma só para as duas mensagens de propósito. Enquanto foram
/// duas, a regra valia onde alguém lembrou de escrevê-la — e a regra existe
/// justamente para o caso em que ninguém está olhando.
///
/// # Errors
///
/// [`MessageRefusal::TooLong`] quando uma das partes passa do teto e
/// [`MessageRefusal::Forbidden`] quando o texto traz o que nunca leva, com o
/// trecho pelo nome e o pedaço em que ele apareceu.
pub fn check_message(
    title: &str,
    body: &str,
    title_max: usize,
    body_max: usize,
) -> Result<(), MessageRefusal> {
    for (part, text, max) in [("title", title, title_max), ("body", body, body_max)] {
        let chars = text.chars().count();
        if chars > max {
            return Err(MessageRefusal::TooLong { part, chars, max });
        }
    }
    let whole = format!("{title}\n{body}");
    let folded = crate::domain::text::fold(&whole);
    for (needle, shown) in FORBIDDEN_IN_MESSAGE {
        if let Some(at) = folded.find(needle) {
            return Err(MessageRefusal::Forbidden {
                found: (*shown).to_string(),
                excerpt: excerpt_around(&folded, at),
            });
        }
    }
    if let Some((email, at)) = first_email(&folded) {
        return Err(MessageRefusal::Forbidden { found: email, excerpt: excerpt_around(&folded, at) });
    }
    Ok(())
}

/// A primeira frase de `text`, sem título de markdown e sem negrito.
///
/// Quem lê a frase é a leitura do índice da spec, a mesma que tira o objetivo
/// do primeiro `context`: uma frase só se lê de um jeito só.
fn first_sentence_of(text: &str) -> String {
    let plain = crate::domain::spec_index::after_titles(text).replace("**", "");
    crate::domain::spec_index::first_sentence(&plain).to_string()
}

/// O título e o corpo do pull request desta spec, montados do arquivo de
/// eventos.
///
/// **Ninguém escreve este texto à mão.** O título sai do objetivo da spec; o
/// corpo sai, em ordem de importância, do resumo que o assistente gravou, de
/// uma linha por onda entregue e da contagem dos critérios com as falhas pelo
/// nome. Quando o corpo passa do teto, as listas viram contagem — o detalhe
/// não se perde, porque ele mora na página da spec.
///
/// # Errors
///
/// A spec sem objetivo escrito ([`MessageRefusal::NoTitle`]), o título acima do
/// teto ([`MessageRefusal::TooLong`], que pede outro objetivo em vez de cortar
/// uma frase ao meio) e o texto que traz o que nunca vai num pull request
/// ([`MessageRefusal::Forbidden`]).
pub fn pr_message(log: &SpecLog) -> Result<(String, String), MessageRefusal> {
    let title = crate::domain::spec_index::goal_of(log).ok_or(MessageRefusal::NoTitle)?;
    let visible = log.visible();
    let summary = visible
        .iter()
        .rev()
        .find(|e| e.event_type == "pr_summary")
        .and_then(|e| e.str_field("text"))
        .unwrap_or_default()
        .trim()
        .to_string();

    let mut waves: BTreeMap<u64, String> = BTreeMap::new();
    for event in &visible {
        if event.event_type != "delivered" {
            continue;
        }
        if let (Some(wave), Some(text)) = (event.wave(), event.str_field("text")) {
            waves.insert(wave, first_sentence_of(text));
        }
    }

    // Quantos critérios existem e quais falharam é leitura do QA, que já mora
    // no núcleo e já decide pela execução mais nova de cada um. Contar aqui de
    // novo seria uma segunda resposta para a mesma pergunta.
    let qa = crate::domain::spec_state::qa(log);
    let criteria = qa.criteria;
    let failed: Vec<String> = qa
        .failed_ids
        .iter()
        .map(|id| {
            visible
                .iter()
                .find(|e| e.id == *id)
                .and_then(|e| e.str_field("when"))
                .map_or_else(|| id.to_string(), first_sentence_of)
        })
        .collect();

    let wave_lines: Vec<String> =
        waves.iter().map(|(wave, what)| format!("- onda {wave}: {what}")).collect();
    let criteria_line = if failed.is_empty() {
        format!("Critérios: {criteria}, nenhum com falha.")
    } else {
        format!("Critérios: {criteria}, {} com falha: {}.", failed.len(), failed.join("; "))
    };

    let assemble = |lines: &[String]| {
        let mut parts: Vec<String> = Vec::new();
        if !summary.is_empty() {
            parts.push(summary.clone());
        }
        if !lines.is_empty() {
            parts.push(lines.join("\n"));
        }
        parts.push(criteria_line.clone());
        parts.join("\n\n")
    };

    let mut body = assemble(&wave_lines);
    if body.chars().count() > MESSAGE_BODY_MAX {
        // As listas viram contagem: o detalhe fica na página da spec, onde
        // nada se perde, e o corpo continua legível de uma olhada.
        let collapsed = vec![format!("Ondas entregues: {}.", waves.len())];
        body = assemble(&collapsed);
    }

    check_message(&title, &body, MESSAGE_TITLE_MAX, MESSAGE_BODY_MAX)?;
    Ok((title, body))
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


    /// Uma spec de teste com 8 ondas e 40 critérios abre um pull request: o
    /// corpo cabe no teto, o título cabe no teto, um título maior é recusado
    /// com a mensagem do limite, e um resumo com link da conversa, o nome do
    /// modelo, um e-mail ou um caminho da máquina é recusado apontando o
    /// trecho.
    #[test]
    fn a_mensagem_do_pull_request_cabe_nos_limites_e_recusa_dado_de_usuario() {
        let mut lines: Vec<String> = Vec::new();
        let mut id = 0u64;
        let mut push = |fields: Value| {
            id += 1;
            let mut map = obj(fields);
            map.insert("v".into(), json!(1));
            map.insert("id".into(), json!(id));
            map.insert("at".into(), json!("2026-09-16T10:00:00-03:00"));
            map.insert("author".into(), json!("assistant"));
            lines.push(render_line(&map));
            id
        };
        push(json!({"type": "context", "text": "Deixar o Mustard enxuto. E o resto da prosa."}));
        for wave in 1..=8u64 {
            push(json!({
                "type": "delivered",
                "wave": wave,
                "files": ["a.rs"],
                // Uma frase só, longa: sem ponto no meio, ela chega inteira ao
                // corpo, e as oito juntas passam do teto — que é o que faz a
                // dobra das listas em contagem ser realmente exercida aqui.
                "text": format!("A onda {wave} entregou {}", "um detalhe e ".repeat(60)),
            }));
        }
        let mut criteria: Vec<u64> = Vec::new();
        for n in 1..=40u64 {
            criteria.push(push(json!({
                "type": "criterion",
                "when": format!("o caso {n} acontece"),
                "then": "a resposta é a combinada",
                "proof": "teste",
            })));
        }
        push(json!({
            "type": "criterion_run",
            "criterion": criteria[3],
            "result": "fail",
            "exit": 1,
            "ms": 12,
        }));
        let resumo = push(json!({"type": "pr_summary", "text": "O portão lê o estado."}));

        let log = parse_log(&lines.join("\n"));
        let (title, body) = pr_message(&log).expect("a spec tem objetivo e resumo");
        assert_eq!(title, "Deixar o Mustard enxuto.");
        assert!(title.chars().count() <= MESSAGE_TITLE_MAX, "título: {}", title.chars().count());
        assert!(
            body.chars().count() <= MESSAGE_BODY_MAX,
            "corpo com {} caracteres: as listas tinham de virar contagem",
            body.chars().count(),
        );
        assert!(body.contains("O portão lê o estado."), "o resumo abre o corpo: {body}");
        assert!(body.contains("Ondas entregues: 8."), "as listas viraram contagem: {body}");
        assert!(body.contains("Critérios: 40"), "os critérios são contados: {body}");
        assert!(body.contains("1 com falha"), "e a falha é nomeada: {body}");

        // Um título maior é recusado com a mensagem do limite.
        let longo = "x".repeat(MESSAGE_TITLE_MAX + 1);
        let refusal = check_message(&longo, "corpo", MESSAGE_TITLE_MAX, MESSAGE_BODY_MAX)
            .expect_err("um título acima do teto é recusado");
        assert_eq!(refusal.reason(), "message-too-long");
        let said = refusal.message(Locale::PtBr);
        assert!(said.contains(&MESSAGE_TITLE_MAX.to_string()), "a recusa diz o limite: {said}");

        // Cada dado de usuário é recusado apontando o trecho.
        for (resumo_ruim, esperado) in [
            ("Veja https://claude.ai/code/x para o resto.", "claude.ai"),
            ("Escrito com a ajuda do Claude.", "Claude"),
            ("Dúvidas com fulano@empresa.com.br.", "fulano@empresa.com.br"),
            ("O arquivo está em /home/fulano/projetos/x.rs.", "/home/"),
        ] {
            let mut com_dado = lines.clone();
            let mut map = obj(json!({"type": "pr_summary", "text": resumo_ruim}));
            map.insert("v".into(), json!(1));
            map.insert("id".into(), json!(resumo + 1));
            map.insert("at".into(), json!("2026-09-16T11:00:00-03:00"));
            map.insert("author".into(), json!("assistant"));
            com_dado.push(render_line(&map));
            let refusal = pr_message(&parse_log(&com_dado.join("\n")))
                .expect_err(&format!("o resumo com `{esperado}` é recusado"));
            assert_eq!(refusal.reason(), "message-forbidden-text", "{esperado}");
            let said = refusal.message(Locale::PtBr);
            assert!(
                said.to_lowercase().contains(&esperado.to_lowercase()),
                "a recusa diz o que achou: {said}",
            );
            assert!(said.contains('"'), "e mostra o trecho em que achou: {said}");
        }
    }

    /// A linha de cada onda no corpo do pull request pula o título — o
    /// cabeçalho e o trecho em negrito sozinho na linha — e fecha a frase
    /// também no ponto de exclamação e no de interrogação.
    ///
    /// Quem lê a primeira frase é a leitura que já existe no pacote. Uma
    /// segunda leitura escrita aqui devolveria o título em negrito inteiro no
    /// lugar da frase, e arrastaria a prosa que vem depois do `!`.
    #[test]
    fn a_linha_da_onda_pula_o_titulo_e_fecha_a_frase_em_qualquer_ponto() {
        let mut lines: Vec<String> = Vec::new();
        let mut id = 0u64;
        let mut push = |fields: Value| {
            id += 1;
            let mut map = obj(fields);
            map.insert("v".into(), json!(1));
            map.insert("id".into(), json!(id));
            map.insert("at".into(), json!("2026-09-16T10:00:00-03:00"));
            map.insert("author".into(), json!("assistant"));
            lines.push(render_line(&map));
        };
        push(json!({"type": "context", "text": "Deixar o Mustard enxuto."}));
        push(json!({
            "type": "delivered",
            "wave": 1,
            "files": ["a.rs"],
            "text": "**O portão de corte**\nA onda parou de perguntar? Depois vem o resto.",
        }));
        push(json!({
            "type": "delivered",
            "wave": 2,
            "files": ["b.rs"],
            "text": "# A prova do vermelho\nFuncionou! E sobrou prosa depois.",
        }));
        push(json!({"type": "pr_summary", "text": "O portão lê o estado."}));

        let log = parse_log(&lines.join("\n"));
        let (_, body) = pr_message(&log).expect("a spec tem objetivo e resumo");
        assert!(
            body.contains("- onda 1: A onda parou de perguntar?"),
            "a linha da onda 1 não pulou o negrito ou não fechou no ponto de \
             interrogação: {body}",
        );
        assert!(
            body.contains("- onda 2: Funcionou!"),
            "a linha da onda 2 não fechou no ponto de exclamação: {body}",
        );
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

    /// O entregou volta para a janela principal a cada onda, e por isso tem
    /// teto: no limite passa, um caractere acima é recusado com o tamanho que
    /// ele tem, nos dois idiomas.
    #[test]
    fn a_delivery_note_over_the_character_cap_is_refused_with_its_size() {
        let note = |chars: usize| {
            json!({"author": "wave", "wave": 1, "text": "á".repeat(chars), "files": ["src/a.rs"]})
        };
        assert!(checked("delivered", note(DELIVERED_MAX_CHARS)).is_ok());
        let refusal = checked("delivered", note(DELIVERED_MAX_CHARS + 1)).unwrap_err();
        assert_eq!(refusal.reason(), "delivered-too-long");
        for lang in [Locale::PtBr, Locale::EnUs] {
            let message = refusal.message(lang);
            assert!(message.contains("8001"), "{message}");
            assert!(message.contains("8000"), "{message}");
        }
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

    /// O que o binário grava não tem mensagem de origem: um critério tirado do
    /// `spec.md` entra sem `origin`, e o mesmo critério pelo assistente, não.
    #[test]
    fn what_the_binary_writes_needs_no_origin() {
        let criterion = json!({"when": "w", "then": "t", "proof": "cargo test"});
        let mut by_binary = criterion.clone();
        by_binary["author"] = json!("binary");
        assert!(checked("criterion", by_binary).is_ok());
        assert_eq!(
            checked("criterion", criterion).unwrap_err(),
            Refusal::MissingField { event_type: "criterion".into(), field: "origin".into() }
        );
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
        assert!(line.ends_with(r#""search":"x k anot not"}"#), "{line}");
        assert!(!shown_line(&event).contains("search"));
    }

    /// Além do texto e das palavras-chave, o campo de busca leva o rótulo, o
    /// nome do tipo em palavras nos dois idiomas, a onda a que o item pertence
    /// e os caminhos dos arquivos que ele cita: procurar pelo nome de um
    /// arquivo acha a tarefa que mexe nele, e procurar por "onda 13" acha o
    /// que é dela.
    #[test]
    fn the_search_of_an_item_carries_its_label_type_wave_and_files() {
        let task = stamp(
            normalize(
                obj(json!({
                    "text": "A rodada grava o que injetou.",
                    "keys": ["envio"],
                    "label": "Onda 13, tarefa 7",
                    "wave": 13,
                    "files": [{"path": "apps/rt/src/commands/flow/round.rs", "new": true}],
                    "origin": 1
                })),
                "task",
            ),
            7,
            None,
            "t",
        );
        let event = SpecEvent { id: 7, event_type: "task".into(), line: 1, fields: task };
        for term in ["round.rs", "commands/flow", "onda 13", "wave 13", "tarefa", "task", "injetou"] {
            assert!(event.matches(&search_terms(term), None), "{term}: {:?}", event.str_field("search"));
        }
        assert!(!event.matches(&search_terms("onda 12"), None), "another wave does not match");
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
        assert_eq!(
            log.get(2).unwrap().str_field("search"),
            Some(search_field(Some("Trava nova."), &["k", "anotação", "note"]).as_str())
        );
        assert_eq!(
            log.get(3).unwrap().str_field("search"),
            Some(search_field(Some("Sem busca gravada."), &["regra", "rule"]).as_str())
        );
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

    /// As ondas entregues são as que têm registro de entrega, e só elas: a
    /// onda que só tem tarefa, e a que só foi enviada, ainda vêm.
    #[test]
    fn the_delivered_waves_are_the_ones_with_a_delivery_record() {
        let content = "{\"v\":1,\"id\":1,\"at\":\"t\",\"type\":\"wave\",\"n\":1,\"text\":\"Uma.\"}\n\
                       {\"v\":1,\"id\":2,\"at\":\"t\",\"type\":\"wave\",\"n\":2,\"text\":\"Duas.\"}\n\
                       {\"v\":1,\"id\":3,\"at\":\"t\",\"type\":\"wave\",\"n\":3,\"text\":\"Três.\"}\n\
                       {\"v\":1,\"id\":4,\"at\":\"t\",\"type\":\"task\",\"wave\":2,\"text\":\"Mexer.\"}\n\
                       {\"v\":1,\"id\":5,\"at\":\"t\",\"type\":\"send\",\"wave\":2,\"text\":\"Pedido.\"}\n\
                       {\"v\":1,\"id\":6,\"at\":\"t\",\"type\":\"delivered\",\"wave\":1,\"text\":\"Saiu.\"}\n\
                       {\"v\":1,\"id\":7,\"at\":\"t\",\"type\":\"delivered\",\"wave\":3,\"text\":\"Saiu.\"}\n";
        let log = parse_log(content);
        assert_eq!(log.delivered_waves(), BTreeSet::from([1, 3]));
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

    /// O ponto que fecha outro nunca fica aberto; o que "não se aplica" leva
    /// o motivo; o `closes` aceita o número ou o código do ponto.
    #[test]
    fn a_closing_point_is_never_open_and_not_applicable_takes_a_reason() {
        let base = json!({"block": "limits", "gap": "g", "from": "gap", "origin": 1, "closes": 3});
        let mut open = base.clone();
        open["status"] = json!("open");
        open["facts"] = json!([{"text": "f", "source": "mensagem 1"}]);
        assert_eq!(checked("point", open), Err(Refusal::ClosingPointOpen));

        let mut skipped = base.clone();
        skipped["status"] = json!("not_applicable");
        skipped["result"] = json!([1]);
        assert_eq!(checked("point", skipped.clone()), Err(Refusal::NotApplicableNeedsReason));
        skipped["reason"] = json!("não vale aqui");
        assert_eq!(checked("point", skipped), Ok(()));

        let mut by_code = base;
        by_code["status"] = json!("closed");
        by_code["result"] = json!([1]);
        by_code["closes"] = json!("MSTD-POINT-0001");
        assert_eq!(checked("point", by_code), Ok(()), "the code the page shows is accepted");
    }
}
