//! Os tipos de evento da spec: os blocos em que cada tipo cai, a forma de
//! cada campo e os 38 tipos, com os campos próprios de cada um.

use serde_json::Value;

use crate::domain::mustard_id;
use crate::platform::i18n::{Locale, translate};

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
pub const METRIC_TYPES: &[&str] = &["injection", "hook", "call", "state", "verdict", "point", "send", "delivered", "stage_run", "validation_failure"];

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
    /// ([`ReadQuery`]): a lista das ondas, o que o pedido da onda lista, o
    /// pedido gravado, o pedido que o fechamento montaria agora, a entrega
    /// vigente, as tarefas por entregar, a soma das chamadas de cada comando
    /// e um item só.
    #[must_use]
    pub fn accepted_names() -> String {
        let mut names: Vec<&str> = Block::ALL.iter().map(|b| b.name()).collect();
        if let Some(i) = names.iter().position(|n| *n == "waves") {
            let beside = [
                "wave-list",
                "wave-<n>",
                "dispatch-<n>",
                "dispatch-review",
                "request-<n>",
                "request-review",
                "request-review-preview",
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
/// fora dos blocos. `wave-list` é uma linha curta por onda — o número, a
/// primeira linha do texto e as ondas de que ela depende —; `dispatch-2` é
/// tudo o que o pedido da onda `2` lista; `dispatch-review`, tudo o que o
/// pedido mais novo do revisor final lista;
/// `request-2`, o pedido exato gravado no envio dela; `request-review`, o
/// pedido exato gravado no último envio do revisor final, que não tem onda;
/// `request-review-preview`, o pedido que o fechamento daria ao revisor final
/// agora, montado sem gravar nada; `delivered-2`, a entrega vigente dela; `backlog`, as tarefas ainda por
/// entregar; `calls`, a soma das chamadas de cada comando; e `item-<código>`
/// ou `item-<número>`, um item só, pelo código ou pela versão de número dado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadQuery {
    Block(BlockQuery),
    WaveList,
    Dispatch(u64),
    ReviewDispatch,
    Request(u64),
    ReviewRequest,
    ReviewPreview,
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
        // Antes de `wave-<n>`, que leria `list` como o número de uma onda.
        if name == "wave-list" {
            return Some(Self::WaveList);
        }
        if name == "dispatch-review" {
            return Some(Self::ReviewDispatch);
        }
        if let Some(rest) = name.strip_prefix("dispatch-") {
            return number(rest).map(Self::Dispatch);
        }
        if name == "request-review" {
            return Some(Self::ReviewRequest);
        }
        if name == "request-review-preview" {
            return Some(Self::ReviewPreview);
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

    /// O nome que esta leitura responde quando `name` é um apelido dela, e
    /// `None` quando `name` já é um nome aceito ou não é apelido de nada.
    ///
    /// Os apelidos são os nomes que o agente chuta: ele pede `tarefas` em vez
    /// de `backlog`, `contexto` em vez de `specification`. Foram cinquenta e
    /// uma recusas assim num projeto de verdade, e cada uma é uma volta
    /// inteira do agente. Entregar o bloco e dizer o nome certo custa nada.
    ///
    /// O apelido se lê sem acento e sem maiúscula, nos dois idiomas: quem
    /// chuta em português chuta `tarefas` e `tarefa`.
    #[must_use]
    pub fn alias_of(name: &str) -> Option<&'static str> {
        let asked = crate::domain::text::fold(name.trim());
        if Self::parse(&asked).is_some() {
            return None;
        }
        ALIASES.iter().find(|(alias, _)| *alias == asked).map(|(_, name)| *name)
    }
}

/// Os nomes que o agente chuta, e o nome aceito de cada um
/// ([`ReadQuery::alias_of`]). Já dobrados: sem acento e em minúsculas.
const ALIASES: &[(&str, &str)] = &[
    ("tarefas", "backlog"),
    ("tarefa", "backlog"),
    ("tasks", "backlog"),
    ("task", "backlog"),
    ("contexto", "specification"),
    ("context", "specification"),
    ("spec", "specification"),
    ("especificacao", "specification"),
    ("ondas", "waves"),
    ("onda", "wave-list"),
    ("wave", "wave-list"),
    ("criterios", "criteria"),
    ("criterio", "criteria"),
    ("notas", "notes"),
    ("nota", "notes"),
    ("nota de obra", "notes"),
    ("revisao", "review"),
    ("estado", "state"),
    ("progresso", "progress"),
    ("conversa", "conversation"),
    ("messages", "conversation"),
    ("mensagens", "conversation"),
    ("combinados", "agreed"),
    ("combinado", "agreed"),
    ("medidas", "metrics"),
    ("metricas", "metrics"),
    ("chamadas", "calls"),
    ("itens", "backlog"),
];

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
/// Por que a tarefa ou o pedido novo é outro trabalho, embora o classificador
/// das atividades o dê como o mesmo de uma tarefa aberta: com o motivo, a
/// gravação passa e o guarda no evento.
const DISTINCT: Field = opt("distinct", Kind::Text);
/// A palavra do usuário de que o pedido é desta spec, embora o Jev o dê como
/// obra de outra spec ou como pendência do projeto: com ela, a gravação
/// passa sem perguntar e a palavra fica no evento.
const SAME_SPEC: Field = opt("same_spec", Kind::Text);
const APPLIES_TO: Field = opt("applies_to", Kind::TextOrObject);
/// As ondas donas de um item combinado, além das ondas das tarefas que o
/// cobrem. O item do projeto diz, em vez disso, que vale no projeto todo
/// (`applies_to` com os arquivos `["**"]`).
const WAVES: Field = opt("waves", Kind::Ints);
/// A marca do item combinado que vale para toda onda: ele vai no pedido de
/// qualquer onda, sem passar pela escolha dos itens, mesmo que diga outros
/// arquivos ou nenhum. Ausente, o item segue o dono que tem.
const EVERY_WAVE: Field = opt("every_wave", Kind::Bool);
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
const MILESTONES: &[&str] = &["approval", "round", "close", "explicit"];
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
/// O que a tarefa de criar ou de atualizar skill fez com a skill: criou,
/// atualizou, manteve — a skill já estava em dia e a versão nova não traz
/// ganho — ou descartou, porque o teste com e sem ela não mudou o resultado;
/// ou a saída e a volta que o binário grava quando a skill deixa de servir e
/// quando ela volta.
const SKILL_ACTIONS: &[&str] = &["created", "updated", "kept", "discarded", "retired", "restored"];
const ROLES: &[&str] = &["wave", "review", "wave-review"];
const VERDICTS: &[&str] = &["approved", "rejected"];
/// O resultado de uma marca da máquina numa onda: a reprovação, que põe a
/// onda em conserto, ou a soltura, que tira a reprovação que não tinha prova.
const FAILURE_RESULTS: &[&str] = &["rejected", "released"];
const EFFECTS: &[&str] = &["new_waves", "adjust_waves"];
const PURGE_REASONS: &[&str] = &["secret", "client_data"];

/// Os 38 tipos. Os campos marcados com `opt` podem faltar; os outros são
/// obrigatórios, e o gravador recusa o evento sem eles.
pub const TYPES: &[TypeSpec] = &[
    // Conversa. A mensagem que responde a um gesto de aprovação leva a
    // testemunha: a pergunta e a opção que o usuário clicou. A que responde a
    // outra pergunta com opções leva a pergunta do assistente em `question`:
    // o texto das duas é só o que o usuário disse. A resposta do
    // assistente aponta a mensagem que respondeu, pelo número ou pelo código
    // dela, e a linha guarda o número; só a do turno em que a spec nasce,
    // antes de qualquer mensagem do usuário, vai sem ela
    // (`spec_state::reply_rule`). A fala do usuário que chega com um ponto do
    // levantamento aberto leva o número dele em `during`: a mensagem já tem
    // lugar no levantamento e não é dita solta.
    ty(
        "message",
        "MSG",
        Block::Conversation,
        false,
        &[TEXT, opt("witness", Kind::Object), opt("during", Kind::Int), opt("question", Kind::Text)],
    ),
    ty("response", "RESP", Block::Conversation, false, &[TEXT, opt("reply_to", Kind::Ref)]),
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
            // A forma do que a ferramenta barrada trazia: cada campo pelo
            // nome e o tamanho dele em caracteres, nunca o conteúdo, que
            // pode ter código do projeto. Sem isso não se sabia depois se a
            // recusa foi justa.
            opt("shape", Kind::Object),
        ],
    ),
    ty(
        "call",
        "CALL",
        Block::Conversation,
        false,
        &[
            req("command", Kind::Text),
            // O bloco que a chamada pediu: o da leitura, o tipo da gravação.
            opt("block", Kind::Text),
            req("ms", Kind::Int),
            req("result", Kind::OneOf(CALL_RESULTS)),
            opt("refusal", Kind::Text),
            // O que a conferência de escrita achou, na recusa `item-unclear`.
            opt("refusal_detail", Kind::Text),
            opt("invocation", Kind::Bool),
            opt("search_enriched", Kind::Bool),
            opt("search_task_evidence", Kind::Bool),
            opt("search_fallback", Kind::Bool),
            opt("search_native_bytes", Kind::Int),
            opt("search_delivered_bytes", Kind::Int),
            opt("input_bytes", Kind::Int),
            opt("output_bytes", Kind::Int),
            opt("estimated_input_tokens", Kind::Int),
            opt("estimated_output_tokens", Kind::Int),
            // Safe gateway/session observations, without code, prompts or keys.
            opt("search_tool", Kind::Text),
            opt("search_purpose", Kind::Text),
            opt("search_choice_gate", Kind::Text),
            opt("search_local_model_calls", Kind::Int),
            opt("search_remote_model_calls", Kind::Int),
            opt("search_selection_status", Kind::Text),
            opt("search_selection_cached", Kind::Bool),
            opt("search_accepted_choices", Kind::Int),
            // Quanto cada etapa da busca levou: a espera pela passada do
            // mapa, pela trava da gravação do que ela aprendeu, os vetores, a
            // investigação e o Jev.
            opt("search_scan_wait_ms", Kind::Int),
            opt("search_lock_wait_ms", Kind::Int),
            opt("search_vectors_ms", Kind::Int),
            opt("search_investigate_ms", Kind::Int),
            opt("search_jev_ms", Kind::Int),
            opt("knowledge_cached", Kind::Bool),
            opt("knowledge_pending", Kind::Bool),
            opt("knowledge_session_model_calls", Kind::Int),
            opt("knowledge_local_model_calls", Kind::Int),
            opt("knowledge_remote_model_calls", Kind::Int),
            opt("knowledge_host", Kind::Text),
            opt("knowledge_model", Kind::Text),
            opt("knowledge_effort", Kind::Text),
            opt("knowledge_effort_observed", Kind::Bool),
            opt("knowledge_input_tokens", Kind::Int),
            opt("knowledge_output_tokens", Kind::Int),
            opt("knowledge_context_target_observed", Kind::Bool),
            opt("knowledge_api_context_tier", Kind::Text),
            // A busca que chamou o filtro: qual (`jev`, ou `jev:<motivo>` na
            // falha), o tempo dele, os tokens de entrada, o custo em
            // milionésimos de dólar (a soma de todos os pedidos), quantos
            // pedidos foram, os candidatos, as peças devolvidas e o modelo
            // que respondeu.
            opt("filter", Kind::Text),
            opt("filter_ms", Kind::Int),
            opt("tokens", Kind::Int),
            opt("cost_micro_usd", Kind::Int),
            opt("requests", Kind::Int),
            opt("candidates", Kind::Int),
            opt("returned", Kind::Int),
            opt("model", Kind::Text),
            // A chamada de reserva do Jev: a chamada cuja decisão o Jev não
            // deu e foi ao agente de análise da sessão.
            opt("reserve_for", Kind::Text),
            // A leitura que o agente faz do pedido dele: de qual pedido é
            // (`request-<onda>` ou `request-review`) e os itens que a leitura
            // achou (o código, ou `lesson-<número>`), todos num recibo só. O
            // recibo de antes, um por item, trazia o item em `item`.
            opt("request", Kind::Text),
            opt("item", Kind::Text),
            opt("items", Kind::Texts),
            // A tarefa da onda que o recibo liberou: a resposta do passo que
            // termina uma tarefa entrega o bloco da seguinte e a libera.
            opt("released", Kind::Text),
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
            // A versão do Mustard em que a spec nasceu: só o nascimento a
            // traz, e é só informação.
            opt("mustard", Kind::Text),
        ],
    ),
    // Native explicit publication receipts. Previous milestone/template
    // fields remain readable for already published historical pages.
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
            opt("provider", Kind::Text),
            opt("deployment_id", Kind::Text),
            opt("snapshot_id", Kind::Text),
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
            opt("result", Kind::Refs),
            opt("reused_results", Kind::Refs),
            opt("reason", Kind::Text),
            opt("reminders", Kind::List),
        ],
    ),
    // Os itens combinados. Cada um tem dono: as ondas (as das tarefas que o
    // cobrem e as que ele diz em `waves`) ou o projeto (`applies_to` no
    // projeto todo).
    ty(
        "rule", "RULE", Block::Agreed, true,
        &[TEXT, KEYS, req("example", Kind::Text), APPLIES_TO, WAVES, EVERY_WAVE, NO_CODE, TITLE, AGENT],
    ),
    ty(
        "limit", "LIMIT", Block::Agreed, true,
        &[TEXT, KEYS, req("value", Kind::Text), APPLIES_TO, WAVES, EVERY_WAVE, NO_CODE, TITLE, AGENT],
    ),
    ty(
        "contract", "CONTR", Block::Agreed, true,
        &[TEXT, KEYS, req("example", Kind::Text), APPLIES_TO, WAVES, EVERY_WAVE, NO_CODE, TITLE, AGENT],
    ),
    ty(
        "error", "ERR", Block::Agreed, true,
        &[TEXT, KEYS, req("message", Kind::Text), APPLIES_TO, WAVES, EVERY_WAVE, NO_CODE, TITLE, AGENT],
    ),
    ty(
        "edge_case", "EDGE", Block::Agreed, true,
        &[TEXT, KEYS, req("expected", Kind::Text), APPLIES_TO, WAVES, EVERY_WAVE, NO_CODE, TITLE, AGENT],
    ),
    ty(
        "out_of_scope", "SCOPE", Block::Agreed, true,
        &[TEXT, KEYS, opt("reason", Kind::Text), APPLIES_TO, WAVES, EVERY_WAVE, NO_CODE, TITLE, AGENT],
    ),
    ty(
        "decision", "DEC", Block::Agreed, true,
        &[TEXT, KEYS, req("why", Kind::Text), APPLIES_TO, WAVES, EVERY_WAVE, NO_CODE, TITLE, AGENT],
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
            opt("contracts", Kind::Refs),
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
            opt("reused", Kind::Bool),
        ],
    ),
    // Measured leaf stages. An enclosing call's duration is inclusive and
    // must not be added to these measurements.
    ty(
        "stage_run",
        "STAGE",
        Block::Progress,
        false,
        &[
            req("phase", Kind::Text),
            req("stage", Kind::Text),
            req("result", Kind::OneOf(&["pass", "fail", "skipped"])),
            req("ms", Kind::Int),
            opt("command", Kind::Text),
            opt("exit", Kind::Int),
            opt("output", Kind::Text),
            opt("fingerprint", Kind::Text),
            opt("scope", Kind::Text),
            opt("version", Kind::Text),
            opt("attempt", Kind::Int),
            opt("criterion", Kind::Int),
            opt("waves", Kind::Ints),
            opt("reused", Kind::Bool),
            // Quantos testes a saída disse ter rodado: a prova no código de
            // antes reaproveitada lê daqui se passou rodando teste.
            opt("tests", Kind::Int),
        ],
    ),
    ty(
        "validation_failure",
        "FAIL",
        Block::Review,
        false,
        &[
            req("wave", Kind::Int),
            req("result", Kind::OneOf(FAILURE_RESULTS)),
            TEXT,
            req("final", Kind::Bool),
            req("fingerprint", Kind::Text),
            opt("files", Kind::Texts),
            opt("reason", Kind::Text),
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
            req("criteria", Kind::Refs),
            req("done_when", Kind::Text),
            opt("depends_on", Kind::Ints),
            // A relação dos itens da onda em ordem de execução. É ela que o
            // pedido leva, uma linha por item; sem ela, os itens saem na
            // ordem do arquivo.
            opt("order", Kind::Ints),
            // O resumo que esta onda continua: o número da entrega de uma onda
            // que parou, com o que ela deixou por fazer. Só a onda que o
            // backlog formou a partir dele o traz; o envio dela o repete.
            opt("summary", Kind::Int),
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
            // Os motivos que a avaliação de uma skill do Mustard soma, cada um
            // com o commit e os arquivos, e a chance que o Jev deu à mudança:
            // a mudança que chega com a avaliação à espera entra numa versão
            // nova dela. Ausente é tarefa comum.
            opt("skill_whys", Kind::Objects),
            // An unresolved external condition or a returned plan that must
            // be revised. A new task version removes this explicit hold.
            opt("blocked_by", Kind::Text),
            opt("retry_guard", Kind::Object),
            opt("covers", Kind::Refs),
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
            // A marca de prioridade: o motivo, em texto, que o usuário deu
            // para a tarefa sair antes das outras. A montagem das ondas solta
            // primeiro a tarefa marcada, sem passar por cima da dependência,
            // dos arquivos de uma onda em andamento nem das vagas. Vazia, a
            // gravação recusa, como todo texto opcional vazio
            // (`check::check_field`). Ausente é tarefa comum.
            opt("priority", Kind::Text),
            // O grupo de assunto da tarefa: `name`, o nome do grupo, e
            // `position`, a ordem dela no grupo. O classificador das
            // atividades o grava na tarefa nova que fica no assunto de uma
            // tarefa aberta, e a onda de um resumo leva de carona as tarefas
            // prontas do grupo dele. Ausente é tarefa sem grupo.
            opt("group", Kind::Object),
            // A regra que a tarefa faz valer: a que o achado que a fez nascer
            // quebrou, pelo código de uma regra da spec ou pelo texto de uma
            // regra nova. O achado de outra volta com a mesma regra entra
            // nela enquanto ela espera no backlog. Ausente é tarefa sem regra.
            opt("rule", Kind::Text),
            // Os lugares da regra que a tarefa faz valer, achados pelo mapa:
            // o arquivo com linha, o arquivo ou o nome de código. A entrega
            // que dá a tarefa por feita muda cada um deles.
            opt("places", Kind::Texts),
            // O motivo de quem grava uma tarefa nova que o classificador das
            // atividades daria como o mesmo trabalho de uma aberta: com ele,
            // a gravação passa sem perguntar e o motivo fica no evento.
            // Ausente, a tarefa nova passa pelo classificador.
            DISTINCT,
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
            // O ganho medido, numa frase; no descarte, o motivo.
            TEXT,
            // A impressão do `SKILL.md`, que o binário grava na criação e na
            // atualização.
            opt("sha", Kind::Text),
            // Os arquivos que a skill cita.
            opt("files", Kind::Texts),
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
            // O que o agente precisa ler antes de entregar: o código de cada
            // item que a linha do pedido lista e `lesson-<número>` de cada
            // lição. A entrega (ou o veredito) só passa com todos lidos; o
            // envio sem este campo é de antes da conferência e não a exige.
            opt("read_items", Kind::Texts),
            // O bloco de cada tarefa da onda, na ordem de execução: o código
            // da tarefa (`task`), o texto (`text`) e o que ele entrega inteiro
            // (`items`). O pedido traz o da primeira; a resposta do passo que
            // termina uma tarefa traz o da seguinte, sem remontar nada.
            opt("tasks", Kind::Objects),
            req("mustard", Kind::Text),
            opt("lessons", Kind::Ints),
            opt("skills", Kind::Objects),
            // A escolha do procedimento por área: só o envio antigo a traz, e
            // nada mais a lê nem a grava.
            opt("expertise", Kind::Object),
            opt("host", Kind::Text),
            // O modelo e o esforço pedidos para o agente, no envio; o que ele
            // usou de verdade, os passos que deu e os tokens que gastou só se
            // sabem na volta, e entram na versão nova do mesmo envio
            // (`replaces`). O consumo de quem despacha até ali — a conta do
            // orquestrador, não da onda — vem junto, na mesma volta.
            opt("model", Kind::Text),
            opt("effort", Kind::Text),
            opt("model_used", Kind::Text),
            // O esforço que valeu no lançamento do agente: o que o gancho do
            // despacho impõe, ou o da variável `CLAUDE_CODE_EFFORT_LEVEL`
            // quando ela está definida no ambiente dele.
            opt("effort_used", Kind::Text),
            // A subida de modelo da onda que voltou reprovada: o modelo do
            // envio anterior (`model`) e o evento que a causou (`cause`) — o
            // veredito que reprovou ou a volta recusada ou reprovada.
            opt("escalated_from", Kind::Object),
            opt("steps", Kind::Int),
            opt("tokens", Kind::Int),
            opt("caller_steps", Kind::Int),
            opt("caller_tokens", Kind::Int),
            opt("usage_breakdown", Kind::Object),
            opt("caller_usage_breakdown", Kind::Object),
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
            // O resumo que o pedido manda ler antes de tudo: o número da
            // entrega de uma onda que parou. Enquanto a onda deste envio não
            // entrega, o resumo está em uso; entregue a onda, está usado.
            opt("summary", Kind::Int),
            // O processo do hospedeiro — o Claude Code ou o Codex — que mandou
            // este envio: o número e a hora de início que `/proc` contava
            // então, para um número reaproveitado não enganar. Sem o par, a
            // rodada não sabe dizer se ele segue aberto, e não dá a onda como
            // órfã; só o envio de versão antiga do Claude Code sem o par segue
            // a regra de antes.
            opt("claude_pid", Kind::Int),
            opt("claude_started", Kind::Int),
            // O pedido do revisor de uma onda (`role` `wave-review`): a onda
            // revisada e a impressão do diff dela que o pedido traz. Fica fora
            // de `wave`, que faria dele o envio da onda.
            opt("reviews", Kind::Int),
            opt("fingerprint", Kind::Text),
            // A marca do envio que saiu sem hospedeiro reconhecido por trás
            // da rodada — um script, um teste, a linha de comando ou um
            // hospedeiro fora da lista —, e por isso sem o par: é o que o
            // separa do envio de versão antiga, também sem o par.
            opt("no_host_process", Kind::Bool),
            // A marca do par que é do hospedeiro reconhecido: o envio de
            // versão antiga feito no Codex traz o par sem ela, porque gravava
            // o processo da própria rodada, e esse par não diz se o
            // hospedeiro segue aberto.
            opt("host_pair", Kind::Bool),
            // A marca da versão que o gancho do despacho grava quando lança o
            // agente da onda: só a onda lançada, com sinal de vida recente,
            // conta como onda com agente trabalhando. O envio novo da rodada
            // sai sem ela.
            opt("agent_launched", Kind::Bool),
            // A reprovação de quem conduz a obra: a volta da onda que ele
            // reprovou, pelo número (`delivered`), e o motivo dele (`reason`).
            // Enquanto essa volta for a última da onda, ela fica fora do
            // commit e a onda espera um agente novo, que recebe o motivo. A
            // versão que grava a reprovação sai sem o Claude Code do envio: a
            // reprovação diz que o agente terminou. A volta nova da onda
            // desfaz a reprovação.
            opt("rejected", Kind::Object),
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
            opt("blocked_by", Kind::Text),
            // Com a mudança de plano: a decisão do usuário que a mudança
            // troca. Toda mudança de plano espera aceite; omitir este campo
            // apenas informa que nenhuma decisão específica foi substituída.
            opt("changes_decision", Kind::Text),
            // A recusa da mudança de plano, que só a rodada grava na entrega
            // oficial: o motivo, com a mensagem do usuário que a sustenta em
            // `origin` — o clique em "Recusar" ou, na recusa de quem conduz, a
            // última mensagem dele. A onda entra no commit sem a mudança.
            opt("replan_refused", Kind::Text),
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
            // Optional reusable conclusions with exact current source receipts.
            opt("knowledge", Kind::Objects),
            // Native tool receipts; free prose remains an agent report.
            opt("observations", Kind::Objects),
            opt("report_status", Kind::OneOf(&["reported-unverified"])),
            // As ondas que um conserto fecha.
            opt("fixes", Kind::Ints),
            // O que o agente achou fora da tarefa e não é dele consertar, cada
            // sobra com título e detalhe (`title`, `detail`), a regra que ela
            // quebrou (`rule`, o código de uma regra da spec ou o texto de uma
            // regra nova) e os lugares dessa regra (`places`), os quatro
            // obrigatórios ([`super::check`]): vira tarefa da spec quando a
            // rodada assume a volta, e os achados da mesma regra viram uma
            // tarefa só. A sobra que só muda comentário,
            // documentação ou texto de ajuda leva a marca `cleanup`, que a
            // tarefa herda.
            opt("leftovers", Kind::Objects),
            // A resposta por cada item combinado que o pedido da onda levou,
            // como a do veredito final (`item`, `met`): o item que não vem
            // cumprido vira tarefa no backlog quando a rodada assume a volta,
            // se nenhuma tarefa ainda por entregar já o cobre e se a análise
            // da onda não o tirou do pedido; com tarefa em `undone`, ele entra
            // na versão dela que volta ao backlog.
            opt("agreed", Kind::Objects),
            // Os lugares da regra de uma tarefa feita que a onda conferiu e
            // deixou sem mudança, cada um com o lugar, como a tarefa o cita,
            // e o motivo (`place`, `reason`): a conferência da entrega não os
            // cobra entre os arquivos mudados.
            opt("checked_places", Kind::Objects),
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
            // Os achados maiores e menores da revisão final, no formato das
            // sobras da entrega de onda (`title`, `detail`, `rule` e `places`,
            // obrigatórios, e `files` e `cleanup`): com o veredito aprovado, o fechamento os grava
            // como tarefas no backlog e manda rodar a rodada antes do pull
            // request; o achado sem regra escrita na spec vai ao usuário como
            // pergunta. Em outro veredito nada lê o campo, e a gravação o
            // recusa.
            opt("leftovers", Kind::Objects),
            // O veredito curto do revisor de uma onda, que a gravação marca:
            // aponta a onda e os arquivos do motivo, sem critério obrigatório.
            opt("wave_review", Kind::Bool),
            opt("files", Kind::Texts),
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
            // As ondas de `waves` que entraram no commit sem mudança nenhuma:
            // o commit delas não teria o que levar, e a rodada as juntou ao
            // anterior. O commit fecha a entrega delas, mas nenhuma linha dele
            // é delas, e a falha numa linha dele não as implica.
            opt("unchanged", Kind::Ints),
        ],
    ),
    ty("pr_summary", "PRSUM", Block::Progress, false, &[TEXT]),
    // Anotações.
    ty(
        "request", "REQ", Block::Notes, true,
        &[TEXT, KEYS, req("effect", Kind::OneOf(EFFECTS)), TITLE, AGENT, DISTINCT, SAME_SPEC],
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
    use crate::domain::spec_events::Refusal;
    use crate::domain::spec_events::tests::checked;

    #[test]
    fn there_are_thirty_eight_types_each_with_one_block() {
        assert_eq!(TYPES.len(), 38);
        let names: BTreeSet<&str> = TYPES.iter().map(|t| t.name).collect();
        assert_eq!(names.len(), 38, "a type name repeats");
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
            "waves, wave-list, wave-<n>, dispatch-<n>, dispatch-review, request-<n>, request-review, request-review-preview, delivered-<n>, backlog, calls, item-<code|n>, review"
        ));
    }

    /// Além dos blocos, a leitura aceita a lista das ondas, o pedido de uma
    /// onda, o pedido gravado, o pedido do revisor final, gravado ou ainda
    /// por montar, a entrega, o backlog, a soma das chamadas e um item pelo código ou pelo número; sem
    /// o número ou com um código fora do formato, o nome não é aceito.
    #[test]
    fn the_readings_beside_the_blocks_take_their_number_or_code() {
        assert_eq!(ReadQuery::parse("state"), Some(ReadQuery::Block(BlockQuery::Block(Block::State))));
        assert_eq!(ReadQuery::parse("wave-3"), Some(ReadQuery::Block(BlockQuery::Wave(3))));
        assert_eq!(ReadQuery::parse(" wave-list "), Some(ReadQuery::WaveList));
        assert_eq!(ReadQuery::parse("dispatch-3"), Some(ReadQuery::Dispatch(3)));
        assert_eq!(ReadQuery::parse(" dispatch-review "), Some(ReadQuery::ReviewDispatch));
        assert_eq!(ReadQuery::parse("request-3"), Some(ReadQuery::Request(3)));
        assert_eq!(ReadQuery::parse(" request-review "), Some(ReadQuery::ReviewRequest));
        assert_eq!(ReadQuery::parse("request-review-preview"), Some(ReadQuery::ReviewPreview));
        assert_eq!(ReadQuery::parse(" review-request-3 "), None, "the wave reviewer is gone");
        assert_eq!(ReadQuery::parse(" delivered-3 "), Some(ReadQuery::Delivered(3)));
        assert_eq!(ReadQuery::parse("backlog"), Some(ReadQuery::Backlog));
        assert_eq!(ReadQuery::parse("calls"), Some(ReadQuery::Calls));
        assert_eq!(ReadQuery::parse("item-42"), Some(ReadQuery::Item(EventRef::Id(42))));
        assert_eq!(
            ReadQuery::parse("item-MSTD-TASK-0003"),
            Some(ReadQuery::Item(EventRef::Code("MSTD-TASK-0003".into())))
        );
        for refused in ["request-", "request-x", "delivered-", "item-", "item-0", "item-tarefa", "backlogs", "call", "wave-lists"] {
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
                "requests": 3, "candidates": 150, "returned": 12, "model": "jev-1.13.0"})),
            Ok(())
        );
        assert_eq!(call(json!({"filter": "jev:no_credit", "filter_ms": 40})), Ok(()));
        assert_eq!(call(json!({"request": "request-273", "item": "MSTD-TASK-0471"})), Ok(()));
        assert_eq!(call(json!({"request": "request-review", "item": "lesson-12"})), Ok(()));
        let refusal = call(json!({"tokens": "muitos"})).unwrap_err();
        assert!(matches!(refusal, Refusal::InvalidValue { ref field, .. } if field == "tokens"), "{refusal:?}");
        let refusal = call(json!({"requests": "vários"})).unwrap_err();
        assert!(matches!(refusal, Refusal::InvalidValue { ref field, .. } if field == "requests"), "{refusal:?}");
        let refusal = call(json!({"item": 7})).unwrap_err();
        assert!(matches!(refusal, Refusal::InvalidValue { ref field, .. } if field == "item"), "{refusal:?}");
    }
}
