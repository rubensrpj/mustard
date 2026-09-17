//! Os tipos de evento da spec: os blocos em que cada tipo cai, a forma de
//! cada campo e os 33 tipos, com os campos próprios de cada um.

use serde_json::Value;

use crate::domain::mustard_id;
use crate::platform::i18n::{translate, Locale};

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
/// retrabalho, os lembretes do levantamento, o tamanho de cada pedido e a
/// entrega que responde a ele.
pub const METRIC_TYPES: &[&str] = &["injection", "hook", "call", "state", "verdict", "point", "send", "delivered"];

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
/// As ondas donas de um item combinado, além das ondas das tarefas que o
/// cobrem. O item do projeto diz, em vez disso, que vale no projeto todo
/// (`applies_to` com os arquivos `["**"]`).
const WAVES: Field = opt("waves", Kind::Ints);
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
const PAGES: &[&str] = &["spec", "project"];
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
    // Os itens combinados. Cada um tem dono: as ondas (as das tarefas que o
    // cobrem e as que ele diz em `waves`) ou o projeto (`applies_to` no
    // projeto todo).
    ty("rule", "RULE", Block::Agreed, true, &[TEXT, KEYS, req("example", Kind::Text), APPLIES_TO, WAVES, NO_CODE]),
    ty("limit", "LIMIT", Block::Agreed, true, &[TEXT, KEYS, req("value", Kind::Text), APPLIES_TO, WAVES, NO_CODE]),
    ty("contract", "CONTR", Block::Agreed, true, &[TEXT, KEYS, req("example", Kind::Text), APPLIES_TO, WAVES, NO_CODE]),
    ty("error", "ERR", Block::Agreed, true, &[TEXT, KEYS, req("message", Kind::Text), APPLIES_TO, WAVES, NO_CODE]),
    ty("edge_case", "EDGE", Block::Agreed, true, &[TEXT, KEYS, req("expected", Kind::Text), APPLIES_TO, WAVES, NO_CODE]),
    ty("out_of_scope", "SCOPE", Block::Agreed, true, &[TEXT, KEYS, opt("reason", Kind::Text), APPLIES_TO, WAVES, NO_CODE]),
    ty("decision", "DEC", Block::Agreed, true, &[TEXT, KEYS, req("why", Kind::Text), APPLIES_TO, WAVES, NO_CODE]),
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
            // A cópia separada que a rodada criou para a onda e a pasta de
            // compilação dela: a volta junta os arquivos da cópia, e a pasta
            // fica ocupada enquanto a onda está em andamento.
            opt("copy", Kind::Text),
            opt("build_dir", Kind::Text),
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
            // A revisão final do conjunto, que o fechamento pede à spec de
            // duas ondas ou mais: aprovada, fica na última onda do plano;
            // reprovada, na onda que o conserto refaz.
            opt("final", Kind::Bool),
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
        &[req("targets", Kind::Refs), req("reason", Kind::OneOf(PURGE_REASONS)), opt("excerpt", Kind::Text)],
    ),
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::json;

    use super::*;
    use crate::domain::spec_events::tests::checked;
    use crate::domain::spec_events::Refusal;

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
}
