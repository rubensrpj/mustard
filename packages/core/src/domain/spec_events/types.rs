//! Os tipos de evento da spec: os blocos em que cada tipo cai, a forma de
//! cada campo e os 36 tipos, com os campos próprios de cada um.

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

/// O tipo do registro que toma o lugar da linha cortada pelo disco cheio
/// ([`repair_cut_lines`](super::repair_cut_lines)). Fica fora de [`TYPES`] de
/// propósito: sem bloco, sem sigla e sem entrada na página, a leitura o aceita
/// sem aviso, o pedido da onda e a página o deixam de fora como item de
/// trabalho, e o gravador não o aceita de quem grava — só o binário o cria.
/// `run read` o mostra quando lido pelo número. O código que a linha cortada
/// trazia continua dela: nenhum outro item o recebe.
pub const CUT_LINE_TYPE: &str = "cut_line";

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

    /// Os nomes aceitos, na ordem da página, para a mensagem de recusa. Com
    /// eles, as leituras que a leitura monta fora dos blocos
    /// ([`ReadQuery`]): o que o pedido da onda lista, o pedido gravado, a
    /// entrega vigente, as tarefas por entregar, a soma das chamadas de cada
    /// comando e um item só.
    #[must_use]
    pub fn accepted_names() -> String {
        let mut names: Vec<&str> = Block::ALL.iter().map(|b| b.name()).collect();
        if let Some(i) = names.iter().position(|n| *n == "waves") {
            let beside = [
                "wave-<n>",
                "dispatch-<n>",
                "request-<n>",
                "request-review",
                "delivered-<n>",
                "backlog",
                "calls",
                "item-<code|n>",
            ];
            for (step, name) in beside.into_iter().enumerate() {
                names.insert(i + 1 + step, name);
            }
        }
        names.join(", ")
    }
}

/// O que o `read` pede: um bloco da spec ou uma das leituras que ele monta
/// fora dos blocos. `dispatch-2` é tudo o que o pedido da onda 2 lista;
/// `request-2`, o pedido exato gravado no envio dela; `request-review`, o
/// pedido exato gravado no último envio do revisor final, que não tem onda;
/// `delivered-2`, a entrega vigente dela; `backlog`, as tarefas ainda por
/// entregar; `calls`, a soma das chamadas de cada comando; e `item-<código>`
/// ou `item-<número>`, um item só, pelo código ou pela versão de número dado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadQuery {
    Block(BlockQuery),
    Dispatch(u64),
    Request(u64),
    ReviewRequest,
    Delivered(u64),
    Backlog,
    Calls,
    Item(EventRef),
}

impl ReadQuery {
    /// Lê o nome pedido; `None` para um nome que não é bloco nem leitura, ou
    /// para uma leitura sem o número ou o código que ela exige.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim();
        let number = |rest: &str| rest.parse::<u64>().ok();
        if let Some(rest) = name.strip_prefix("dispatch-") {
            return number(rest).map(Self::Dispatch);
        }
        if name == "request-review" {
            return Some(Self::ReviewRequest);
        }
        if let Some(rest) = name.strip_prefix("request-") {
            return number(rest).map(Self::Request);
        }
        if let Some(rest) = name.strip_prefix("delivered-") {
            return number(rest).map(Self::Delivered);
        }
        if let Some(rest) = name.strip_prefix("item-") {
            return match number(rest) {
                Some(id) => (id > 0).then_some(Self::Item(EventRef::Id(id))),
                None => EventRef::from_value(&Value::String(rest.to_string())).map(Self::Item),
            };
        }
        if name == "backlog" {
            return Some(Self::Backlog);
        }
        if name == "calls" {
            return Some(Self::Calls);
        }
        BlockQuery::parse(name).map(Self::Block)
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
    /// Um destes números.
    OneOfNumbers(&'static [u64]),
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
            Self::OneOfNumbers(numbers) => value.as_u64().is_some_and(|n| numbers.contains(&n)),
            Self::TextOrObject => value.is_string() || value.is_object(),
            Self::Time => value.as_str().is_some_and(is_time_prefix),
        }
    }

    /// A forma em palavras, para a recusa.
    #[must_use]
    pub fn describe(self, lang: Locale) -> String {
        let (key, values) = match self {
            Self::Text => ("spec_events.kind.text", None),
            Self::Int => ("spec_events.kind.int", None),
            Self::Bool => ("spec_events.kind.bool", None),
            Self::Object => ("spec_events.kind.object", None),
            Self::Ints => ("spec_events.kind.ints", None),
            Self::Texts => ("spec_events.kind.texts", None),
            Self::Objects => ("spec_events.kind.objects", None),
            Self::List => ("spec_events.kind.list", None),
            Self::OneOf(w) => ("spec_events.kind.one_of", Some(w.join(", "))),
            Self::ManyOf(w) => ("spec_events.kind.many_of", Some(w.join(", "))),
            Self::OneOfNumbers(n) => (
                "spec_events.kind.one_of_numbers",
                Some(n.iter().map(u64::to_string).collect::<Vec<_>>().join(", ")),
            ),
            Self::TextOrObject => ("spec_events.kind.text_or_object", None),
            Self::Time => ("spec_events.kind.time", None),
            Self::Ref => ("spec_events.kind.ref", None),
            Self::Refs => ("spec_events.kind.refs", None),
        };
        let base = translate(key, lang);
        values.map_or_else(|| base.to_string(), |v| base.replace("{values}", &v))
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
    /// inteiro, como `MSTD-RULE-NNNN`. `None` para qualquer outra coisa.
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
/// O nome curto do item, que o usuário lê antes de tudo. Opcional no tipo
/// porque o item antigo não tem; o gravador o exige do item que o modelo
/// grava (`spec_events::write::record_in`).
const TITLE: Field = opt("title", Kind::Text);
/// A parte do agente: os arquivos, as linhas, os comandos e o que testar, em
/// markdown curto. O `text` fica com o porquê, para o usuário. Opcional no
/// tipo pelo mesmo motivo do título.
const AGENT: Field = opt("agent", Kind::Text);
const APPLIES_TO: Field = opt("applies_to", Kind::TextOrObject);
/// As ondas donas de um item combinado, além das ondas das tarefas que o
/// cobrem. O item do projeto diz, em vez disso, que vale no projeto todo
/// (`applies_to` com os arquivos `["**"]`).
const WAVES: Field = opt("waves", Kind::Ints);
/// O item combinado que não vira código: o valor é o motivo. Quem o traz sai
/// do aviso dos itens sem tarefa, porque não há tarefa que o implemente.
const NO_CODE: Field = opt("no_code", Kind::Text);
/// A volta que o próprio agente grava — a entrega da onda ou o veredito do
/// revisor —, com `true`. Ela fica fora da leitura até a rodada ou o
/// fechamento a assumir, gravando a versão oficial, sem o campo, com
/// `replaces` para ela.
const RETURNED: Field = opt("returned", Kind::Bool);

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
/// As cinco formas do padrão de critério de aceitação: a que vale sempre, a
/// disparada por um acontecimento, a que só vale enquanto um estado durar, a
/// que só vale se um recurso existir, e a que trata um acontecimento
/// indesejado. Obrigatória na gravação nova (`check::check_conditions`);
/// critério já gravado sem o campo continua válido na leitura.
const CRITERION_FORMS: &[&str] =
    &["ubiquitous", "event_driven", "state_driven", "optional_feature", "unwanted_behavior"];
const SKILL_ACTIONS: &[&str] = &["create", "change", "drop"];
const ROLES: &[&str] = &["wave", "review"];
const VERDICTS: &[&str] = &["approved", "rejected"];
const EFFECTS: &[&str] = &["new_waves", "adjust_waves"];
const PURGE_REASONS: &[&str] = &["secret", "client_data"];

/// Os 36 tipos. Os campos marcados com `opt` podem faltar; os outros são
/// obrigatórios, e o gravador recusa o evento sem eles.
pub const TYPES: &[TypeSpec] = &[
    // Conversa. A mensagem que responde a um gesto de aprovação leva a
    // testemunha: a pergunta e a opção que o usuário clicou. A resposta do
    // assistente aponta a mensagem que respondeu; só a do turno em que a spec
    // nasce, antes de qualquer mensagem do usuário, vai sem ela
    // (`spec_state::reply_rule`). A fala do usuário que chega com um ponto do
    // levantamento aberto leva o número dele em `during`: a mensagem já tem
    // lugar no levantamento e não é dita solta.
    ty("message", "MSG", Block::Conversation, false, &[TEXT, opt("witness", Kind::Object), opt("during", Kind::Int)]),
    ty("response", "RESP", Block::Conversation, false, &[TEXT, opt("reply_to", Kind::Int)]),
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
            // A busca que chamou o filtro: qual (`jev`, ou `jev:<motivo>` na
            // falha), o tempo dele, os tokens de entrada, o custo em
            // milionésimos de dólar, os candidatos, as peças devolvidas e o
            // modelo que respondeu.
            opt("filter", Kind::Text),
            opt("filter_ms", Kind::Int),
            opt("tokens", Kind::Int),
            opt("cost_micro_usd", Kind::Int),
            opt("candidates", Kind::Int),
            opt("returned", Kind::Int),
            opt("model", Kind::Text),
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
    // A publicação de uma página. A do template do Mustard, que lê o banco de
    // dados guardado junto da página, traz `template: true`; a que não traz é
    // a página inteira de uma versão antiga, que fica parada como está. O
    // `stamp` é o carimbo do molde publicado, a versão do layout dele e a
    // impressão do conteúdo: só a versão do layout conta. O molde que o
    // programa rodando monta com outra versão de layout, ou a publicação com o
    // carimbo de antes dela (a versão do Mustard) ou sem carimbo, não manda
    // publicar de novo: o marco avisa o usuário, uma vez por versão, e grava o
    // aviso como uma publicação que não aconteceu (`ok` falso e o motivo
    // `layout-changed`). A página só se publica de novo, no mesmo endereço,
    // quando o usuário pede. A mesma versão de layout com outra impressão não
    // muda nada.
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
            opt("template", Kind::Bool),
            opt("stamp", Kind::Text),
        ],
    ),
    // A cópia dos itens para o banco de dados de uma página publicada, gravada
    // depois que ela foi feita: a da página da spec diz em `last` o número do
    // último item que ela levou; a da página do projeto diz em `phase` a fase
    // da linha da spec que ela levou. Em `versions`, a versão que o banco
    // devolveu a cada documento escrito, pelo nome `coleção/documento`
    // (`ranges/200`, `computed/current`, `specs/<spec>`): a cópia seguinte a
    // põe em `if_version` na troca dele, sem ler a versão antes.
    ty(
        "copy",
        "COPY",
        Block::State,
        false,
        &[
            req("page", Kind::OneOf(PAGES)),
            opt("last", Kind::Int),
            opt("phase", Kind::OneOf(PHASES)),
            opt("versions", Kind::Object),
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
    ty(
        "rule", "RULE", Block::Agreed, true,
        &[TEXT, KEYS, req("example", Kind::Text), APPLIES_TO, WAVES, NO_CODE, TITLE, AGENT],
    ),
    ty(
        "limit", "LIMIT", Block::Agreed, true,
        &[TEXT, KEYS, req("value", Kind::Text), APPLIES_TO, WAVES, NO_CODE, TITLE, AGENT],
    ),
    ty(
        "contract", "CONTR", Block::Agreed, true,
        &[TEXT, KEYS, req("example", Kind::Text), APPLIES_TO, WAVES, NO_CODE, TITLE, AGENT],
    ),
    ty(
        "error", "ERR", Block::Agreed, true,
        &[TEXT, KEYS, req("message", Kind::Text), APPLIES_TO, WAVES, NO_CODE, TITLE, AGENT],
    ),
    ty(
        "edge_case", "EDGE", Block::Agreed, true,
        &[TEXT, KEYS, req("expected", Kind::Text), APPLIES_TO, WAVES, NO_CODE, TITLE, AGENT],
    ),
    ty(
        "out_of_scope", "SCOPE", Block::Agreed, true,
        &[TEXT, KEYS, opt("reason", Kind::Text), APPLIES_TO, WAVES, NO_CODE, TITLE, AGENT],
    ),
    ty(
        "decision", "DEC", Block::Agreed, true,
        &[TEXT, KEYS, req("why", Kind::Text), APPLIES_TO, WAVES, NO_CODE, TITLE, AGENT],
    ),
    // Especificação.
    ty("context", "CTX", Block::Specification, true, &[TEXT, TITLE, AGENT]),
    ty("concern", "CONC", Block::Specification, true, &[TEXT, TITLE, AGENT]),
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
            // Obrigatório na gravação nova, exigido em
            // `check::check_conditions` com a recusa que lista as cinco
            // formas pelo nome; `opt` aqui só para não entrar na recusa
            // genérica de campo ausente, sem a lista. Critério gravado antes
            // desta exigência continua sem o campo, e a leitura não recusa.
            opt("form", Kind::OneOf(CRITERION_FORMS)),
            opt("contracts", Kind::Ints),
            // Só o título: `when` e `then` já são a parte do usuário, e
            // `proof` é a do agente.
            TITLE,
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
            // O número da onda é opcional: a tarefa pode nascer sem ele e
            // ganhá-lo depois, no plano.
            opt("wave", Kind::Int),
            TEXT,
            // O nome curto da tarefa, que diz o que ela entrega.
            TITLE,
            AGENT,
            // A tarefa sem arquivo que já se sabe qual é declara a lista
            // vazia; a ausência do campo é outra coisa, e o gravador a
            // recusa (`spec_events::write::record_in`), junto da falta de
            // `depends_on`.
            opt("files", Kind::Objects),
            opt("skill", Kind::Text),
            opt("covers", Kind::Ints),
            opt("must_read", Kind::List),
            // As tarefas de que esta depende, pelo número ou pelo código;
            // vazia quando não depende de nenhuma. Alimenta a ordem das
            // ondas (topológica) e, como `files`, é obrigatória na gravação.
            opt("depends_on", Kind::Refs),
            // A tarefa de limpeza: nasceu de uma sobra que só muda
            // comentário, documentação ou texto de ajuda, e a rodada a segura
            // até o fim da obra, para sair junto das outras numa onda só.
            // Ausente é tarefa comum.
            opt("cleanup", Kind::Bool),
            // O par de papéis cuja direção de importação esta tarefa muda,
            // por pedido formal do usuário: a conferência depois da onda
            // libera a importação entre os dois só nos arquivos desta
            // tarefa. Ausente, a regra forte vale como sempre.
            opt("role_pair", Kind::Texts),
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
            // A onda é opcional: o envio do pedido da revisão final, que não
            // é dono de onda nenhuma, grava sem ela — a mesma porta que a
            // rodada usa para o pedido de cada onda.
            opt("wave", Kind::Int),
            req("role", Kind::OneOf(ROLES)),
            // O nome do agente que a onda chamou: `wave`, o mesmo em toda
            // onda. O envio antigo pode trazer `wave-solo`, o agente de
            // tarefa única que foi juntado ao `wave`; o reenvio dele chama o
            // `wave`. O molde em si não é gravado — ele mora no projeto,
            // igual para todo envio.
            opt("agent", Kind::Text),
            // O molde do agente, como o instalador o gravou no projeto: só o
            // envio antigo o traz, e nada mais o lê — o reenvio dele chama o
            // `wave`, como todo envio.
            opt("template", Kind::Text),
            // O pedido exato, como foi injetado no agente; nada aqui é
            // remontado na leitura.
            TEXT,
            req("lines", Kind::Int),
            req("chars", Kind::Int),
            // Os itens do pedido: a onda leva os dela, cada um pelo número.
            // O pedido da revisão final não recorta itens — cobre o
            // combinado inteiro — e sai sem este campo.
            opt("items", Kind::Ints),
            req("mustard", Kind::Text),
            opt("lessons", Kind::Ints),
            opt("skills", Kind::Objects),
            // O modelo e o esforço pedidos para o agente, no envio; o que ele
            // usou de verdade, os passos que deu e os tokens que gastou só se
            // sabem na volta, e entram na versão nova do mesmo envio
            // (`replaces`). O consumo de quem despacha até ali — a conta do
            // orquestrador, não da onda — vem junto, na mesma volta.
            opt("model", Kind::Text),
            opt("effort", Kind::Text),
            opt("model_used", Kind::Text),
            opt("steps", Kind::Int),
            opt("tokens", Kind::Int),
            opt("caller_steps", Kind::Int),
            opt("caller_tokens", Kind::Int),
            // A cópia separada que a rodada criou para a onda e a pasta de
            // compilação dela: a volta junta os arquivos da cópia, e a pasta
            // fica ocupada enquanto a onda está em andamento. No envio da
            // revisão final, a vaga que o fechamento preparou para o
            // revisor, ocupada até o veredito.
            opt("copy", Kind::Text),
            opt("build_dir", Kind::Text),
            // A escolha do orquestrador antes do envio, à parte dos itens que
            // ficaram (`items`): os itens julgados (`judged`), os do projeto
            // todo que saíram (`removed`) e os sem dono que entraram
            // (`added`), cada um como `{"item": <número>, "why": "<o motivo
            // numa frase>"}`; e as lições do banco julgadas
            // (`judged_lessons`) e as que saíram (`removed_lessons`), cada uma
            // como `{"lesson": <número no banco>, "why": "<o motivo>"}`.
            opt("analysis", Kind::Object),
            // O envio anterior, pelo número ou pelo código: só num reenvio,
            // da onda pausada ou da órfã de um Claude Code que fechou.
            opt("resends", Kind::Ref),
            // O processo do Claude Code que mandou este envio — o número e a
            // hora de início que `/proc` contava então, para um número
            // reaproveitado não enganar. Sem o par, num envio de versão
            // antiga ou fora do Linux, a rodada não sabe dizer se ele segue
            // aberto.
            opt("claude_pid", Kind::Int),
            opt("claude_started", Kind::Int),
        ],
    ),
    ty(
        "delivered",
        "DELIV",
        Block::Waves,
        false,
        &[
            req("wave", Kind::Int),
            TEXT,
            // A lista de arquivos é opcional: a onda que só foi conferir volta
            // sem mexer em nenhum, e o texto dela diz o que conferiu.
            opt("files", Kind::Texts),
            opt("replan", Kind::Text),
            // O código de cada tarefa da onda que o agente não fez. Com a
            // mudança de plano, é obrigatório, vazio quando fez todas; sem
            // ela, ausente quer dizer que fez todas. Cada tarefa citada volta
            // ao backlog quando a rodada assume a volta.
            opt("undone", Kind::Texts),
            // O resumo do commit, em palavras, de onde a rodada monta o título.
            opt("commit", Kind::Text),
            // As provas dos testes de nome novo, cada uma com o critério e o
            // comando (`criterion`, `proof`).
            opt("proofs", Kind::Objects),
            // As ondas que um conserto fecha.
            opt("fixes", Kind::Ints),
            // O que o agente achou fora da tarefa e não é dele consertar, cada
            // sobra com título e detalhe (`title`, `detail`): vira tarefa da
            // spec quando a rodada assume a volta. A sobra que só muda
            // comentário, documentação ou texto de ajuda leva a marca
            // `cleanup`, que a tarefa herda.
            opt("leftovers", Kind::Objects),
            // A resposta por cada item combinado que o pedido da onda levou,
            // como a do veredito final (`item`, `met`): o item que não vem
            // cumprido vira tarefa no backlog quando a rodada assume a volta,
            // se nenhuma tarefa ainda por entregar já o cobre e se a análise
            // da onda não o tirou do pedido.
            opt("agreed", Kind::Objects),
            RETURNED,
        ],
    ),
    // O agente de onda grava um passo ao terminar cada tarefa e ao provar o
    // vermelho e o verde de cada critério: não substitui a entrega do fim.
    ty(
        "step",
        "STEP",
        Block::Waves,
        false,
        &[req("wave", Kind::Int), req("item", Kind::Ref), TEXT],
    ),
    // Revisão.
    ty(
        "verdict",
        "VERD",
        Block::Review,
        false,
        &[
            // A onda que o veredito julga. O agente de teste dedicado que
            // aprova a obra inteira não aponta uma onda; a cobrança do campo
            // fica com a situação (veja `check_conditions`).
            opt("wave", Kind::Int),
            req("result", Kind::OneOf(VERDICTS)),
            TEXT,
            // Os critérios conferidos. A revisão de uma onda os traz sempre; a
            // revisão final do conjunto não confere critério nenhum, e a
            // cobrança do campo fica com a situação (veja `check_conditions`).
            opt("criteria", Kind::Objects),
            opt("lessons", Kind::Objects),
            // A revisão final do conjunto, que o fechamento pede a toda obra:
            // fica sem onda, na resposta por todo o combinado vigente que
            // `agreed` traz, item a item — cobrança de fora, junto do
            // veredito (veja `check_conditions`).
            opt("final", Kind::Bool),
            opt("agreed", Kind::Objects),
            RETURNED,
        ],
    ),
    // A tabela de rastreabilidade que a aceitação do veredito final grava:
    // uma linha por item do combinado, com o item apontado, a verificação e
    // o arquivo que o próprio veredito já trazia por item, e a situação. O
    // binário grava sozinho, a partir do que a revisão final respondeu — não
    // é uma resposta livre de quem revisa.
    ty("tracking", "TRACK", Block::Review, false, &[req("items", Kind::Objects)]),
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
    ty(
        "request", "REQ", Block::Notes, true,
        &[TEXT, KEYS, req("effect", Kind::OneOf(EFFECTS)), TITLE, AGENT],
    ),
    ty("deferred", "DEFER", Block::Notes, true, &[TEXT, KEYS, req("pending", Kind::Int)]),
    ty(
        "note", "NOTE", Block::Notes, true,
        &[TEXT, KEYS, TITLE, AGENT],
    ),
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
    fn there_are_thirty_six_types_each_with_one_block() {
        assert_eq!(TYPES.len(), 36);
        let names: BTreeSet<&str> = TYPES.iter().map(|t| t.name).collect();
        assert_eq!(names.len(), 36, "a type name repeats");
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
        assert!(BlockQuery::accepted_names().contains(
            "waves, wave-<n>, dispatch-<n>, request-<n>, request-review, delivered-<n>, backlog, calls, item-<code|n>, review"
        ));
    }

    /// Além dos blocos, a leitura aceita o pedido de uma onda, o pedido
    /// gravado, o pedido do revisor final, a entrega, o backlog, a soma das chamadas e um item pelo
    /// código ou pelo número; sem o número ou com um código fora do formato,
    /// o nome não é aceito.
    #[test]
    fn the_readings_beside_the_blocks_take_their_number_or_code() {
        assert_eq!(ReadQuery::parse("state"), Some(ReadQuery::Block(BlockQuery::Block(Block::State))));
        assert_eq!(ReadQuery::parse("wave-3"), Some(ReadQuery::Block(BlockQuery::Wave(3))));
        assert_eq!(ReadQuery::parse("dispatch-3"), Some(ReadQuery::Dispatch(3)));
        assert_eq!(ReadQuery::parse("request-3"), Some(ReadQuery::Request(3)));
        assert_eq!(ReadQuery::parse(" request-review "), Some(ReadQuery::ReviewRequest));
        assert_eq!(ReadQuery::parse(" delivered-3 "), Some(ReadQuery::Delivered(3)));
        assert_eq!(ReadQuery::parse("backlog"), Some(ReadQuery::Backlog));
        assert_eq!(ReadQuery::parse("calls"), Some(ReadQuery::Calls));
        assert_eq!(ReadQuery::parse("item-42"), Some(ReadQuery::Item(EventRef::Id(42))));
        assert_eq!(
            ReadQuery::parse("item-MSTD-TASK-0003"),
            Some(ReadQuery::Item(EventRef::Code("MSTD-TASK-0003".into())))
        );
        for refused in ["request-", "request-x", "delivered-", "item-", "item-0", "item-tarefa", "backlogs", "call"] {
            assert_eq!(ReadQuery::parse(refused), None, "{refused}");
        }
    }

    /// A tarefa leva, quando o pedido formal muda o desenho, o par de papéis
    /// cuja direção muda, como uma lista de nomes; outro formato é recusado.
    #[test]
    fn a_task_takes_the_pair_of_roles_its_request_changes() {
        let task = |pair: serde_json::Value| {
            checked("task", json!({"text": "Mudar a direção.", "origin": 1, "files": [], "depends_on": [],
                "role_pair": pair}))
        };
        assert_eq!(task(json!(["service", "controller"])), Ok(()));
        let refusal = task(json!("service")).unwrap_err();
        assert!(matches!(refusal, Refusal::InvalidValue { ref field, .. } if field == "role_pair"), "{refusal:?}");
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

    /// A chamada da busca com filtro leva o filtro, o tempo dele, os tokens,
    /// o custo, os candidatos, as peças e o modelo, todos opcionais: sem
    /// eles, a chamada de sempre continua valendo; com um número em texto, a
    /// recusa diz o campo.
    #[test]
    fn a_call_takes_the_filter_fields_and_still_holds_without_them() {
        let call = |extra: serde_json::Value| {
            let mut fields = json!({"author": "binary", "command": "map search", "ms": 1800, "result": "ok"});
            if let (Some(fields), Some(extra)) = (fields.as_object_mut(), extra.as_object()) {
                fields.extend(extra.clone());
            }
            checked("call", fields)
        };
        assert_eq!(call(json!({})), Ok(()));
        assert_eq!(
            call(json!({"filter": "jev", "filter_ms": 1500, "tokens": 20985, "cost_micro_usd": 881,
                "candidates": 100, "returned": 12, "model": "jev-1.13.0"})),
            Ok(())
        );
        assert_eq!(call(json!({"filter": "jev:no_credit", "filter_ms": 40})), Ok(()));
        let refusal = call(json!({"tokens": "muitos"})).unwrap_err();
        assert!(matches!(refusal, Refusal::InvalidValue { ref field, .. } if field == "tokens"), "{refusal:?}");
    }
}
