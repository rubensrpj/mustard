//! A conversa da spec atual e o que o programa observou de si enquanto ela
//! corria. A mensagem do usuário, a resposta do assistente e o recibo de
//! leitura de um pedido são do trabalho e vão para a spec. O que um gancho fez
//! e cada chamada de comando são observação e vão para o arquivo de operações
//! da spec; quem os lê junta esse arquivo ao que as specs antigas gravaram na
//! própria spec ([`observed`]). Os dois arquivos só recebem a linha nova, com
//! a trava, sem que o que já está neles seja analisado de novo.
//!
//! Quem grava é o binário, nunca o modelo: a mensagem e a resposta chegam
//! pelo Claude Code, e o despachante sabe o que cada gancho decidiu, com o
//! autor de cada um.
//!
//! Só se grava numa spec atual, que tem arquivo de eventos e ainda não
//! terminou: sem ela, nada é gravado, menos o custo da chamada que usou o
//! Jev, que vai para o arquivo das chamadas soltas da máquina e segue
//! contando no teto do mês. Nenhuma gravação daqui falha para quem chama —
//! uma recusa ou um erro de disco vira "nada gravado".
//!
//! A mensagem, com a testemunha quando há, e a resposta são gravadas já sem
//! segredo: cada trecho com cara de chave, token ou senha sai como "…", a
//! mesma marca do expurgo. O
//! segredo colado na conversa nunca entra no arquivo da spec, e o expurgo
//! fica só como reserva.

use std::path::Path;
use std::time::Instant;

use mustard_core::domain::spec_events::{Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::{SpecState, State, last_user_message};
use mustard_core::domain::survey;
use mustard_core::io::spec_events as store;
use mustard_core::io::spend;
use serde_json::{Map, Value, json};

use crate::shared::secret::without_secrets;
use crate::shared::spec_state::DiskSpecState;

/// As fases em que a spec já terminou, e a conversa não é mais dela.
const FINISHED: &[&str] = &["delivered", "discarded"];

/// O que um gancho fez, quando barrou ou avisou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookAction {
    Warn,
    Block,
}

impl HookAction {
    fn name(self) -> &'static str {
        match self {
            Self::Warn => "warn",
            Self::Block => "block",
        }
    }
}

/// A spec que recebe a conversa de `session` no projeto em `root`: a que a
/// abertura ou a retomada ligou à sessão, quando ela tem arquivo de eventos e
/// não terminou. A sessão sem ligação não tem spec: a conversa paralela na
/// mesma branch não grava na obra da outra.
///
/// A fase sai da linha da spec no índice, que toda gravação refaz com a trava
/// da spec presa: ler o arquivo de eventos inteiro só para saber se a spec
/// terminou custava uma leitura completa a cada gancho que agia e a cada
/// busca. Sem a linha no índice, vale o arquivo de eventos.
pub(crate) fn conversation_spec(root: &Path, session: Option<&str>) -> Option<String> {
    use mustard_core::io::spec_events as store;
    let spec = crate::shared::spec_state::conversation_bound_spec(&root.to_string_lossy(), session)?;
    let base = store::spec_root(root);
    let events = store::spec_file(&base, &spec).ok().filter(|path| path.is_file())?;
    let phase = match mustard_core::io::spec_index::read(&base).into_iter().find(|line| line.name == spec) {
        Some(line) => line.phase,
        None => State::from_log(&store::read(&events).ok()??).phase.map(str::to_string),
    };
    (!phase.is_some_and(|phase| FINISHED.contains(&phase.as_str()))).then_some(spec)
}

/// Grava no arquivo de operações da spec da conversa o que um gancho fez; o
/// número do evento lá, ou `None` quando não há spec ou a gravação foi
/// recusada.
fn record(root: &Path, session: Option<&str>, event_type: &str, draft: Map<String, Value>) -> Option<u64> {
    let spec = conversation_spec(root, session)?;
    observe(root, &spec, event_type, vec![draft])?.first().copied()
}

/// Grava `drafts` no arquivo de operações da spec `spec`, que precisa ter
/// arquivo de eventos: só acrescenta as linhas, numa escrita só e com a
/// trava do arquivo, sem as conferências do trabalho, que a observação não
/// muda, e sem analisar o que já está nele; passando do teto, a parte antiga
/// vai ao arquivo morto ao lado ([`store::append_operations`]). Os números
/// gravados, ou `None` sem a spec, na spec de antes desta versão do Mustard,
/// que fica como está, ou com a gravação recusada.
pub(crate) fn observe(root: &Path, spec: &str, event_type: &str, drafts: Vec<Map<String, Value>>) -> Option<Vec<u64>> {
    let base = store::spec_root(root);
    store::spec_file(&base, spec).ok().filter(|path| path.is_file())?;
    if mustard_core::io::legacy_specs::is_legacy(&base, spec) {
        return None;
    }
    let path = store::operations_file(root, spec)?;
    let session = crate::shared::spec_state::session_from_env();
    let drafts = drafts.into_iter().map(|mut draft| {
        // De qual sessão veio a chamada, o bloqueio ou a injeção: sem isso o
        // consumo de duas conversas no mesmo projeto saía somado.
        if !draft.contains_key("session")
            && let Some(session) = &session
        {
            draft.insert("session".into(), Value::String(session.clone()));
        }
        (event_type, draft)
    });
    store::append_operations(&path, drafts.collect()).ok()
}

/// Acrescenta um evento ao fim da spec `spec`, com a trava dela e sem
/// analisar o arquivo inteiro ([`store::append`]): a fala, a resposta e o
/// recibo de leitura só crescem no fim e não mudam a fase, o objetivo nem o
/// levantamento que as regras da gravação do `run write` conferem. `guard`
/// recebe o arquivo, lido pelos envelopes, com a trava presa. Numa pasta do
/// formato antigo, numa spec de antes desta versão do Mustard, ou sem o
/// arquivo de eventos, nada é gravado. O evento leva a sessão `session` que o
/// gravou, quando se sabe.
fn record_in_spec(
    root: &Path,
    spec: &str,
    session: Option<&str>,
    event_type: &str,
    mut draft: Map<String, Value>,
    guard: impl FnOnce(&SpecLog) -> Result<(), Refusal>,
) -> Option<u64> {
    let base = store::spec_root(root);
    if super::pages::old_format_spec(&base, spec) || mustard_core::io::legacy_specs::is_legacy(&base, spec) {
        return None;
    }
    if let Some(session) = session.filter(|session| !session.trim().is_empty()) {
        draft.entry("session").or_insert_with(|| Value::String(session.to_string()));
    }
    let path = store::spec_file(&base, spec).ok().filter(|path| path.is_file())?;
    store::append(&path, vec![(event_type, draft)], guard).ok()?.first().copied()
}

/// Os tipos do que o programa observa de si: o que um gancho fez e cada
/// chamada de comando.
const OBSERVED: &[&str] = &["injection", "hook", "call"];

/// O evento é uma observação do programa e não do trabalho: a chamada que
/// leva o pedido (`request`) é o recibo de leitura, que a entrega confere, e
/// fica de fora.
fn is_observation(event: &SpecEvent) -> bool {
    OBSERVED.contains(&event.event_type.as_str()) && !event.fields.contains_key("request")
}

/// A hora de `event` como número, para ordenar eventos de arquivos
/// diferentes; a hora que não se lê vem antes de todas.
pub(crate) fn time_of(event: &SpecEvent) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(event.at()).ok().and_then(|at| at.timestamp_nanos_opt())
}

/// O que o programa observou de si na spec `spec`, cujo arquivo de eventos é
/// `log`: o que os ganchos fizeram e as chamadas de comando, menos os
/// recibos de leitura. Junta as duas fontes — o que as specs antigas
/// gravaram no próprio arquivo de eventos e o arquivo de operações —, na
/// ordem da hora. Nenhuma spec antiga é reescrita: quem lê passa por aqui.
pub(crate) fn observed(root: &Path, spec: &str, log: &SpecLog) -> Vec<SpecEvent> {
    let operations = recorded_operations(root, spec);
    let mut events: Vec<SpecEvent> =
        log.visible().into_iter().chain(operations.visible()).filter(|event| is_observation(event)).cloned().collect();
    events.sort_by_key(time_of);
    events
}

fn draft(fields: Value) -> Map<String, Value> {
    match fields {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// Grava a fala do usuário na spec da conversa. Com um ponto do levantamento
/// aberto, ela leva o número dele em `during`: a fala dita no meio de um ponto
/// tem lugar nele e não fica solta no fim do levantamento. É o ponto que o
/// levantamento está apresentando, o primeiro dos abertos. A fala sem ponto
/// aberto vai sem o campo.
fn record_user_message(root: &Path, session: Option<&str>, mut fields: Value) -> Option<u64> {
    let spec = conversation_spec(root, session)?;
    let open = DiskSpecState::new(root).log(&spec).and_then(|log| survey::open_points(&log).first().map(|p| p.id));
    if let Some(point) = open {
        fields["during"] = json!(point);
    }
    record_in_spec(root, &spec, session, "message", draft(fields), |_| Ok(()))
}

/// A mensagem do usuário, como ele a escreveu, sem os segredos.
pub(crate) fn record_message(root: &Path, session: Option<&str>, text: &str) -> Option<u64> {
    if text.trim().is_empty() {
        return None;
    }
    let text = without_secrets(text);
    record_user_message(root, session, json!({ "author": "user", "text": text }))
}

/// As mensagens que o usuário digitou antes de a spec `spec` nascer, gravadas
/// nela em ordem, como o gancho grava a mensagem do turno: autor usuário,
/// gravadas pelo programa e sem os segredos. Quem chama é a abertura, logo
/// depois do nascimento, quando a spec ainda não tem mensagem nenhuma; as
/// mensagens seguintes ficam com o gancho. Cada uma leva a sessão
/// `session` da conversa. Devolve os números gravados.
pub(crate) fn record_earlier_messages(root: &Path, spec: &str, session: Option<&str>, texts: &[String]) -> Vec<u64> {
    texts
        .iter()
        .filter(|text| !text.trim().is_empty())
        .filter_map(|text| {
            let fields = draft(json!({ "author": "user", "text": without_secrets(text) }));
            record_in_spec(root, spec, session, "message", fields, |_| Ok(()))
        })
        .collect()
}

/// A resposta do usuário a uma pergunta de gesto, com a testemunha: a
/// pergunta, a opção que ele clicou e, quando a pergunta decide uma mudança,
/// o código dela ao lado da resposta. É o registro que o fluxo lê como o
/// "sim" do usuário — pelo código, nunca pela frase mostrada —, e só a
/// testemunha o grava. O texto, a pergunta e a opção vão sem os segredos,
/// como a mensagem: a nota digitada ao lado da opção pode trazer uma senha
/// ou uma chave. O código da mudança vai como veio.
pub(crate) fn record_witnessed_message(root: &Path, session: Option<&str>, text: &str, question: &str, answer: &str, change: Option<&str>) -> Option<u64> {
    if text.trim().is_empty() {
        return None;
    }
    let mut witness = json!({ "question": without_secrets(question), "answer": without_secrets(answer) });
    if let Some(code) = change {
        witness["change"] = json!(code);
    }
    let fields = json!({ "author": "user", "text": without_secrets(text), "witness": witness });
    record_user_message(root, session, fields)
}

/// A resposta do usuário a uma pergunta sem testemunha — a pergunta comum, ou
/// o texto digitado em "Outro" —, com a pergunta do assistente em
/// `question`, fora do texto: o texto é só o que o usuário disse. As duas vão
/// sem os segredos, como a mensagem.
pub(crate) fn record_answered_message(root: &Path, session: Option<&str>, text: &str, question: &str) -> Option<u64> {
    if text.trim().is_empty() {
        return None;
    }
    let fields = json!({ "author": "user", "text": without_secrets(text), "question": without_secrets(question) });
    record_user_message(root, session, fields)
}

/// A resposta do assistente ao fim do turno, ligada à última mensagem do
/// usuário. No turno em que a spec nasce ainda não há mensagem do usuário
/// gravada nela: a resposta vai sem `reply_to`, e o sim à sugestão feita ali
/// acha a resposta que respondeu. A resposta que a regra das pendências
/// barrou também passa por aqui, e o complemento que o bloqueio pediu vem
/// depois dela, ligado à mesma mensagem. A resposta que repete um segredo é
/// gravada sem ele, como a mensagem. A fala já gravada antes de um clique não
/// se repete: com o mesmo texto da última resposta, sem diferença de espaços,
/// e só com mensagens testemunhadas depois dela, nada é gravado.
pub(crate) fn record_response(root: &Path, session: Option<&str>, text: &str) -> Option<u64> {
    reply(root, session, text, |after| !after.is_empty() && after.iter().all(|message| message.fields.contains_key("witness")))
}

/// A fala do assistente no turno de uma pergunta com opções, lida da conversa
/// antes de gravar o clique: ela só viraria resposta no fim do turno, e quem
/// lê a última resposta antes do clique precisa dela já gravada. Nada é
/// gravado quando a última resposta já tem o mesmo texto, sem diferença de
/// espaços.
pub(crate) fn record_said_before_click(root: &Path, session: Option<&str>, text: &str) -> Option<u64> {
    reply(root, session, text, |_| true)
}

/// Grava a resposta `text`, ligada à última mensagem do usuário, a menos que
/// a última resposta gravada tenha o mesmo texto, sem diferença de espaços,
/// e `repeated` diga, pelas mensagens gravadas depois dela, que é a mesma
/// fala.
fn reply(root: &Path, session: Option<&str>, text: &str, repeated: impl FnOnce(&[&SpecEvent]) -> bool) -> Option<u64> {
    if text.trim().is_empty() {
        return None;
    }
    let spec = conversation_spec(root, session)?;
    let log = DiskSpecState::new(root).log(&spec)?;
    let text = without_secrets(text);
    if messages_after_the_same_response(&log, &text).is_some_and(|after| repeated(&after)) {
        return None;
    }
    let mut fields = json!({ "author": "assistant", "text": text });
    let asked = last_user_message(&log).map(|asked| asked.id);
    if let Some(asked) = asked {
        fields["reply_to"] = json!(asked);
    }
    // O `reply_to` sai da leitura de antes da trava. Uma mensagem do usuário
    // gravada depois dela, com a trava, deixa a resposta sem `reply_to` de
    // fora, como a regra da resposta faz na gravação completa: a resposta sem
    // ele nunca fica depois de uma mensagem do usuário.
    let seen = log.max_id();
    let asked_since = |now: &SpecLog| {
        now.events.iter().any(|e| e.id > seen && e.event_type == "message" && e.str_field("author").map(str::trim) == Some("user"))
    };
    record_in_spec(root, &spec, session, "response", draft(fields), |now| match asked {
        None if asked_since(now) => Err(Refusal::MissingField { event_type: "response".to_string(), field: "reply_to".to_string() }),
        _ => Ok(()),
    })
}

/// As mensagens gravadas depois da última resposta de `log`, quando ela tem
/// o texto `text`, sem diferença de espaços; `None` quando não tem, ou não há
/// resposta.
fn messages_after_the_same_response<'a>(log: &'a SpecLog, text: &str) -> Option<Vec<&'a SpecEvent>> {
    let events = log.visible();
    let at = events.iter().rposition(|event| event.event_type == "response")?;
    let same = events[at].str_field("text").is_some_and(|last| last.split_whitespace().eq(text.split_whitespace()));
    same.then(|| events[at + 1..].iter().copied().filter(|event| event.event_type == "message").collect())
}

/// Um gancho barrou ou avisou: quem, o quê, em que ferramenta (ou evento) e
/// por quê.
pub(crate) fn record_hook(root: &Path, session: Option<&str>, hook: &str, action: HookAction, tool: &str, reason: &str) -> Option<u64> {
    record_hook_shaped(root, session, hook, action, tool, reason, None)
}

/// [`record_hook`] com a forma do que a ferramenta barrada trazia
/// (`shape`): cada campo pelo nome e o tamanho dele em caracteres, nunca o
/// conteúdo. É o que diz, depois, se a recusa foi justa.
pub(crate) fn record_hook_shaped(
    root: &Path,
    session: Option<&str>,
    hook: &str,
    action: HookAction,
    tool: &str,
    reason: &str,
    shape: Option<Value>,
) -> Option<u64> {
    let mut fields = json!({ "author": "hook", "hook": hook, "action": action.name(), "tool": tool, "reason": reason });
    if let Some(shape) = shape.filter(|shape| shape.as_object().is_some_and(|fields| !fields.is_empty())) {
        fields["shape"] = shape;
    }
    record(root, session, "hook", draft(fields))
}

/// A forma de `input`, para o registro de um bloqueio: cada campo do objeto
/// pelo nome e o tamanho dele em caracteres, nunca o conteúdo, que pode
/// trazer código do projeto. `None` quando não é objeto ou está vazio.
///
/// O tamanho de um texto é o número de caracteres dele; o de qualquer outro
/// valor, o do JSON que o escreve. Nome de campo não é conteúdo: ele vem do
/// contrato da ferramenta, e é justamente o que falta saber na recusa.
pub(crate) fn shape_of(input: &Value) -> Option<Value> {
    let fields = input.as_object()?;
    if fields.is_empty() {
        return None;
    }
    let sizes: Map<String, Value> = fields
        .iter()
        .map(|(name, value)| {
            let size = match value.as_str() {
                Some(text) => text.chars().count(),
                None => value.to_string().chars().count(),
            };
            (name.clone(), json!(size))
        })
        .collect();
    Some(Value::Object(sizes))
}

/// Um gancho colocou texto na conversa: quem, o texto e o tamanho dele em
/// caracteres.
pub(crate) fn record_injection(root: &Path, session: Option<&str>, hook: &str, text: &str) -> Option<u64> {
    record(root, session, "injection", draft(json!({ "author": "hook", "hook": hook, "chars": text.chars().count(), "text": text })))
}

/// Uma chamada de um passo do fluxo: o comando, quanto tempo levou, se deu
/// certo, e a razão da recusa (`reason`, ou o `error` das portas do pull
/// request). A spec é a do relatório, a que a chamada nomeou, ou a atual.
pub(crate) fn record_call(root: &Path, command: &str, named: Option<&str>, started: Instant, report: &Value) -> Option<u64> {
    let output=serde_json::to_string_pretty(report).unwrap_or_default()+"\n";
    record_output_call(root,command,named,started,report,&output)
}

/// Record the actual output when a command returns plain text or compact
/// JSON. Do not substitute a synthetic JSON object for a byte measurement.
pub(crate) fn record_output_call(root: &Path, command: &str, named: Option<&str>, started: Instant, report: &Value, output:&str) -> Option<u64> {
    let session = crate::shared::spec_state::session_from_env();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut measured=transport_metrics(&args,output);
    measured.insert("invocation".into(),json!(true));
    if command=="search" {
        measured.insert("search_enriched".into(),json!(report["crossing_status"]=="enriched"));
        measured.insert("search_task_evidence".into(),json!(report["task_context"]["cards"].as_array().is_some_and(|cards|!cards.is_empty())));
        measured.insert("search_fallback".into(),json!(report["task_context"]["status"]=="native-fallback" || report["crossing_status"]!="enriched"),
        );
        if report["delivery_measurement"]["basis"] == "same-request-stdout-bytes" {
            for field in ["native_bytes", "delivered_bytes"] {
                if let Some(value) = report["delivery_measurement"][field].as_u64() {
                    measured.insert(format!("search_{field}"), json!(value));
                }
            }
        }
        // Safe decision metadata only: never persist intent, code, candidates or keys.
        for (field,value) in search_decisions(report) {measured.insert(field,value);}
    }
    if command=="knowledge" {
        // Caller-reported usage is separate from Jev billing and host
        // telemetry. Never persist excerpts, prompt text or receipt contents.
        for (field,path) in [("cached","/cached"),("pending","/pending"),("session_model_calls","/session_model_calls"),
            ("local_model_calls","/local_model_calls"),("remote_model_calls","/remote_model_calls"),
            ("host","/host"),("model","/model"),("effort","/effort"),("effort_observed","/effort_observed"),("input_tokens","/input_tokens"),("output_tokens","/output_tokens"),
            ("context_target_observed","/context/target_observed"),("api_context_tier","/context/api_reference/tier")] {
            if let Some(value)=report.pointer(path).filter(|v|!v.is_null()) {measured.insert(format!("knowledge_{field}"),value.clone());}
        }
    }
    record_measured_call(root, command, named, session.as_deref(), started, report, measured)
}

fn search_decisions(report:&Value)->Map<String,Value> {
    let mut fields=Map::new();
    for (field,path) in [("search_tool","/tool"),("search_purpose","/purpose"),
        ("search_choice_gate","/choice_gate"),("search_local_model_calls","/task_context/local_model_calls"),
        ("search_remote_model_calls","/remote_model_calls"),("search_selection_status","/task_context/selection/status"),
        ("search_selection_cached","/task_context/selection/cached"),("search_accepted_choices","/task_context/selection/accepted_choices"),
        // Onde o tempo da busca foi, etapa por etapa.
        ("search_scan_wait_ms","/timings/scan_wait_ms"),("search_lock_wait_ms","/timings/lock_wait_ms"),("search_vectors_ms","/timings/vectors_ms"),
        ("search_investigate_ms","/timings/investigate_ms"),("search_jev_ms","/timings/jev_ms")] {
        if let Some(value)=report.pointer(path).filter(|value|!value.is_null()) {fields.insert(field.into(),value.clone());}
    }
    fields
}

fn transport_metrics(args:&[String],output:&str)->Map<String,Value> {
    let mut measured=Map::new();
    // Count the transport, never persist arguments or response contents.
    // Four characters per token is an explicitly labelled approximation.
    if args.first().is_some_and(|arg| arg == "run") {
        let input = args.join(" ");
        measured.insert("input_bytes".into(), json!(input.len()));
        measured.insert("output_bytes".into(), json!(output.len()));
        measured.insert("estimated_input_tokens".into(), json!(input.chars().count().div_ceil(4)));
        measured.insert("estimated_output_tokens".into(), json!(output.chars().count().div_ceil(4)));
    }
    measured
}

/// A chamada de [`record_call`], da sessão `session`, com os campos da
/// medida de quem chamou (`measured`) além dos de toda chamada: a busca do
/// mapa grava o filtro, o tempo dele, os tokens e o custo. A busca por
/// palavra, que é parte compartilhada e não alcança este módulo, recebe esta
/// função de quem monta a cena dela (`crate::shared::word_search::Record`).
/// A chamada que usou o Jev e não ficou no arquivo de operações de uma spec,
/// por não haver uma ou pela gravação recusada, deixa os tokens e o custo
/// dele no arquivo das chamadas soltas da máquina. Esses registros não
/// representam saldo do provedor.
pub(crate) fn record_measured_call(
    root: &Path,
    command: &str,
    named: Option<&str>,
    session: Option<&str>,
    started: Instant,
    report: &Value,
    mut measured: Map<String, Value>,
) -> Option<u64> {
    measured.entry("invocation").or_insert(json!(false));
    let billed = billing(&measured);
    let recorded = record_in_call_spec(root, command, named, session, started, report, measured);
    if recorded.is_none() {
        record_loose(root, billed);
    }
    recorded
}

/// Os tokens e o custo do Jev que a medida `measured` traz; `None` sem
/// nenhum dos dois.
fn billing(measured: &Map<String, Value>) -> Option<(u64, u64)> {
    let tokens = measured.get("tokens").and_then(Value::as_u64);
    let cost = measured.get("cost_micro_usd").and_then(Value::as_u64);
    (tokens.is_some() || cost.is_some()).then(|| (tokens.unwrap_or(0), cost.unwrap_or(0)))
}

/// Deixa no arquivo das chamadas soltas da máquina os tokens e o custo do
/// Jev de uma chamada que não ficou no arquivo de operações de uma spec.
fn record_loose(root: &Path, billed: Option<(u64, u64)>) {
    if let (Some((tokens, cost)), Some(dir)) = (billed, spend::machine_dir()) {
        let _ = spend::record_loose_call(&dir, root, tokens, cost);
    }
}

/// O arquivo de operações da spec `spec`, lido com o arquivo morto ao lado
/// ([`store::read_operations`]): o histórico inteiro; vazio sem ele.
pub(crate) fn recorded_operations(root:&Path,spec:&str)->mustard_core::domain::spec_events::SpecLog {
    store::operations_file(root,spec).and_then(|path|store::read_operations(&path).ok().flatten()).unwrap_or_default()
}

fn project_operations_path(root:&Path)->std::path::PathBuf {
    mustard_core::io::spec_events::spec_root(root).join(".claude/mustard/operations/.project.ndjson")
}

/// A gravação de [`record_measured_call`] no arquivo de operações da spec do
/// relatório, da que a chamada nomeou ou da atual; `None` sem nenhuma delas
/// ou com a gravação recusada. Toda chamada é observação do programa e mora
/// ali, fora da spec: ela nunca gasta número de item, nunca muda uma gravação
/// recusada e nunca se passa pelo recibo de leitura ([`record_receipt`]). A
/// spec só precisa ter arquivo de eventos, e basta conferir o caminho: ler o
/// arquivo inteiro para isso custava uma leitura completa por chamada.
///
/// A chamada de uma sessão sem ligação neste projeto não fica em spec
/// nenhuma: nem a da conversa paralela na mesma branch, nem a leitura que uma
/// conversa faz com `--root` de outro projeto.
fn record_in_call_spec(
    root: &Path,
    command: &str,
    named: Option<&str>,
    session: Option<&str>,
    started: Instant,
    report: &Value,
    measured: Map<String, Value>,
) -> Option<u64> {
    if session.is_some()
        && crate::shared::spec_state::conversation_bound_spec(&store::spec_root(root).to_string_lossy(), session).is_none()
    {
        return None;
    }
    let spec = report
        .get("spec")
        .and_then(Value::as_str)
        .or(named)
        .map(str::trim)
        .filter(|spec| !spec.is_empty())
        .map(str::to_string)
        .or_else(|| conversation_spec(root, session));
    if let Some(spec) = &spec {
        store::spec_file(&store::spec_root(root), spec).ok().filter(|path| path.is_file())?;
    } else if command != "knowledge" {
        return None;
    }
    let fields = call_fields(command, started, report, measured);
    let Some(spec) = spec else {
        return store::append_operations(&project_operations_path(root), vec![("call", fields)]).ok()?.first().copied();
    };
    observe(root, &spec, "call", vec![fields])?.first().copied()
}

/// O recibo da leitura de um pedido: uma chamada `read` com o pedido
/// (`request-<onda>` ou `request-review`) e todos os itens que a leitura
/// achou, gravada na spec `spec` numa gravação só. É do trabalho, e não
/// observação: a entrega da onda e o veredito da revisão final conferem por
/// ele que o agente leu o que o pedido lista. Um recibo por item regravava a
/// spec inteira a cada um.
pub(crate) fn record_receipt(root: &Path, spec: &str, request: &str, items: &[String], started: Instant) -> Option<u64> {
    let measured = draft(json!({ "invocation": false, "request": request, "items": items }));
    let session = crate::shared::spec_state::session_from_env();
    record_in_spec(root, spec, session.as_deref(), "call", call_fields("read", started, &json!({ "ok": true }), measured), |_| Ok(()))
}

/// O recibo de [`record_receipt`] da tarefa `task` que a resposta do passo
/// entregou inteira: os itens do bloco dela contam como lidos, e `released`
/// diz que ela foi liberada.
pub(crate) fn record_release(root: &Path, spec: &str, request: &str, task: &str, items: &[String], started: Instant) -> Option<u64> {
    let measured = draft(json!({ "invocation": false, "request": request, "items": items, "released": task }));
    let session = crate::shared::spec_state::session_from_env();
    record_in_spec(root, spec, session.as_deref(), "call", call_fields("read", started, &json!({ "ok": true }), measured), |_| Ok(()))
}

/// Os campos de toda chamada `command`, feita em `started`: o autor, o tempo,
/// o resultado do relatório `report` e, na recusa, a razão dela; depois, os
/// da medida de quem chamou (`measured`).
fn call_fields(command: &str, started: Instant, report: &Value, measured: Map<String, Value>) -> Map<String, Value> {
    let ok = report.get("ok").and_then(Value::as_bool) == Some(true);
    let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut fields = json!({
        "author": "binary",
        "command": command,
        "ms": ms,
        "result": if ok { "ok" } else { "refused" },
    });
    let reason = report.get("reason").or_else(|| report.get("error")).and_then(Value::as_str);
    if let Some(reason) = reason.filter(|_| !ok) {
        fields["refusal"] = json!(reason);
    }
    // O que a conferência de escrita achou, na recusa dela ou entre as
    // recusas juntas: sem isso, só se sabia que o item foi recusado. O
    // defeito cita o começo da frase do usuário, e a frase pode abrir com uma
    // chave: o detalhe grava sem ela, como os outros campos.
    let defects: Vec<&str> = std::iter::once(report)
        .chain(report.get("issues").and_then(Value::as_array).into_iter().flatten())
        .filter_map(|issue| issue.get("defects").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !ok && !defects.is_empty() {
        fields["refusal_detail"] = json!(without_secrets(&defects.join("; ")));
    }
    let mut fields = draft(fields);
    fields.extend(measured);
    fields
}

/// As chamadas `command` que saíram juntas em `started`, da spec `spec`, cada
/// uma com a medida dela (`measured`), com os campos que
/// [`record_measured_call`] grava numa: todas no arquivo de operações numa
/// gravação só, que só acrescenta as linhas. Sem a spec, ou com a gravação
/// recusada, nenhuma fica, e o custo de cada uma que usou o Jev vai para as
/// chamadas soltas da máquina. Os números gravados, na ordem.
pub(crate) fn record_measured_calls(root: &Path, spec: &str, command: &str, started: Instant, measured: Vec<Map<String, Value>>) -> Option<Vec<u64>> {
    let spec = spec.trim();
    let report = json!({ "ok": true, "spec": spec });
    let mut billed = Vec::with_capacity(measured.len());
    let fields: Vec<Map<String, Value>> = measured
        .into_iter()
        .map(|mut call| {
            call.entry("invocation").or_insert(json!(false));
            billed.push(billing(&call));
            call_fields(command, started, &report, call)
        })
        .collect();
    let recorded = if spec.is_empty() { None } else { observe(root, spec, "call", fields) };
    if recorded.is_none() {
        for call in billed {
            record_loose(root, call);
        }
    }
    recorded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::record_open;
    use crate::shared::spec_state::stand_on_spec_branch;
    use mustard_core::domain::spec_state::PhaseWriter;

    #[test]
    fn transport_counts_use_real_utf8_output_and_store_no_content_or_credentials() {
        let args:Vec<String>=["run","answer","--json","secret-value"].into_iter().map(str::to_string).collect();
        let output="Pedido exato 🟡\n";
        let measure=transport_metrics(&args,output);
        assert_eq!(measure["output_bytes"],output.len());
        assert_eq!(measure["estimated_output_tokens"],output.chars().count().div_ceil(4));
        assert!(!json!(measure).to_string().contains("secret-value"));
        assert!(transport_metrics(&[],output).is_empty(),"library calls have unknown CLI payloads");
    }

    #[test]
    fn same_query_volume_is_recorded_without_copying_search_results() {
        let dir = project_on("medicao");
        let report = json!({"ok":true,"private_result":"private-code",
            "delivery_measurement":{"basis":"same-request-stdout-bytes","native_bytes":100,"delivered_bytes":150}});
        assert!(record_output_call(dir.path(), "search", Some("medicao"), Instant::now(), &report, "private-code").is_some());
        let log = recorded_operations(dir.path(), "medicao");
        assert_eq!(log.events.last().unwrap().int("search_native_bytes"), Some(100));
        assert_eq!(log.events.last().unwrap().int("search_delivered_bytes"), Some(150));
        assert!(!format!("{:?}", log.events).contains("private-code"));
    }

    #[test]
    fn the_time_of_each_search_stage_reaches_the_operations_log() {
        let dir = project_on("medicao");
        let report = json!({"ok":true,"timings":{"scan_wait_ms":3000,"lock_wait_ms":200,"vectors_ms":40,"investigate_ms":900,"jev_ms":0}});
        assert!(record_output_call(dir.path(), "search", Some("medicao"), Instant::now(), &report, "result").is_some());
        let log = recorded_operations(dir.path(), "medicao");
        let call = log.events.last().unwrap();
        for (field, ms) in [("scan_wait_ms", 3000), ("lock_wait_ms", 200), ("vectors_ms", 40), ("investigate_ms", 900), ("jev_ms", 0)] {
            assert_eq!(call.int(&format!("search_{field}")), Some(ms), "{field}: {:?}", call.fields);
        }
    }

    #[test]
    fn transport_observations_are_separate_and_required_reading_receipts_stay_in_the_spec() {
        let dir=project_on("medicao");
        let root=dir.path();
        let path=mustard_core::io::spec_events::spec_file(root,"medicao").unwrap();
        let original=std::fs::read(&path).unwrap();
        let refused=json!({"ok":false,"spec":"medicao","reason":"invalid-value"});
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| assert!(record_output_call(root,"write",Some("medicao"),Instant::now(),&refused,"refused\n").is_some()));
            }
        });
        assert_eq!(std::fs::read(&path).unwrap(),original,"observations cannot change spec items or consume IDs");
        let observed=recorded_operations(root,"medicao");
        assert_eq!(observed.events.len(),8,"concurrent observations use the locked writer");
        assert!(observed.events.iter().all(|e|e.fields["invocation"]==true && e.str_field("result")==Some("refused")));
        assert!(record_receipt(root,"medicao","request-1",&["MSTD-RULE-0001".to_string()],Instant::now()).is_some());
        let receipts=events_of(root,"medicao","call");
        assert_eq!(receipts.len(),1);
        assert_eq!((&receipts[0]["request"],&receipts[0]["items"]),(&json!("request-1"),&json!(["MSTD-RULE-0001"])));
        assert_eq!(recorded_operations(root,"medicao").events.len(),8);
        assert!(record_call(root,"open",Some("medicao"),Instant::now(),&json!({"ok":true,"kind":"feature"})).is_some());
        assert_eq!(events_of(root,"medicao","call").len(),1,"a flow step is an observation too");
        assert_eq!(recorded_operations(root,"medicao").events.len(),9);
    }

    #[test]
    fn search_decision_metadata_keeps_native_local_cache_and_remote_outcomes_without_payloads() {
        let report=json!({"tool":"Grep","purpose":"implement","choice_gate":"eligible","intent":"private prompt",
            "remote_model_calls":0,"task_context":{"local_model_calls":1,"cards":["private code"],
                "selection":{"status":"jev-choice","cached":true,"accepted_choices":1,"key":"private key"}}});
        let metrics=search_decisions(&report);
        assert_eq!(metrics["search_local_model_calls"],1);
        assert_eq!(metrics["search_remote_model_calls"],0);
        assert_eq!(metrics["search_choice_gate"],"eligible");
        assert_eq!(metrics["search_selection_cached"],true);
        assert!(!serde_json::to_string(&metrics).unwrap().contains("private"));
        let unknown=search_decisions(&json!({"remote_model_calls":null}));
        assert!(unknown.is_empty(),"historical unknown usage must not become measured zero");
    }

    #[test]
    fn knowledge_usage_is_separate_from_billing_and_keeps_no_source_payload() {
        let dir=project_on("interpretacao");
        let report=json!({"ok":true,"host":"codex","model":"gpt-6-luna","session_model_calls":1,
            "input_tokens":100_001,"output_tokens":null,"remote_model_calls":0,"cached":false,
            "context":{"target_observed":false,"api_reference":{"tier":"standard"}},
            "interpretation":{"text":"private code sentinel"},"request":{"prompt":"private prompt sentinel"}});
        assert!(record_output_call(dir.path(),"knowledge",Some("interpretacao"),Instant::now(),&report,&report.to_string()).is_some());
        assert!(events_of(dir.path(),"interpretacao","call").is_empty(),"observations do not consume spec IDs");
        let log=recorded_operations(dir.path(),"interpretacao");
        let fields=&log.events[0].fields;
        assert_eq!(fields["knowledge_input_tokens"],100_001);
        assert_eq!(fields["knowledge_api_context_tier"],"standard");
        assert_eq!(fields["knowledge_context_target_observed"],false);
        for absent in ["tokens","cost_micro_usd","knowledge_output_tokens","interpretation","request"] {assert!(!fields.contains_key(absent),"{absent}");}
        let text=json!(fields).to_string();
        assert!(!text.contains("private code sentinel") && !text.contains("private prompt sentinel"));
    }

    /// Um projeto com a spec `spec` aberta e o checkout na branch dela.
    fn project_on(spec: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").expect("config");
        stand_on_spec_branch(root, spec);
        record_open(root, spec, &format!("feature/{spec}"), "dev").expect("open");
        // A conversa dos ganchos destes testes abriu a spec.
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), "s1", spec);
        dir
    }

    fn events_of(root: &Path, spec: &str, event_type: &str) -> Vec<Map<String, Value>> {
        DiskSpecState::new(root)
            .log(spec)
            .map(|log| log.visible().into_iter().filter(|e| e.event_type == event_type).map(|e| e.fields.clone()).collect())
            .unwrap_or_default()
    }

    /// Com spec atual, a mensagem e a resposta ligada a ela vão para o bloco
    /// da conversa, cada uma com o autor certo.
    #[test]
    fn the_conversation_lands_in_the_current_spec() {
        let dir = project_on("conversa");
        let root = dir.path();
        let asked = record_message(root, None, "arrume o login").expect("message recorded");
        record_response(root, None, "Arrumei.").expect("response recorded");

        let message = &events_of(root, "conversa", "message")[0];
        assert_eq!((message["author"].as_str(), message["text"].as_str()), (Some("user"), Some("arrume o login")));
        let response = &events_of(root, "conversa", "response")[0];
        assert_eq!(response["reply_to"], json!(asked));
        assert_eq!(response["author"], json!("assistant"));
    }

    /// O que um gancho fez — barrar, avisar, colocar texto — vai para o
    /// arquivo de operações da spec atual, e a spec fica como estava: ela
    /// guarda o trabalho, e cada gancho que agia regravava o arquivo inteiro.
    /// O bloqueio guarda a forma do que a ferramenta trazia: cada campo pelo
    /// nome e o tamanho dele, nunca o conteúdo, que pode ter código do
    /// projeto. Sem a forma não se sabia, depois, se a recusa foi justa.
    #[test]
    fn a_block_records_the_shape_of_what_the_tool_carried() {
        let shape = shape_of(&json!({"command": "ls -la", "reason": "ver", "quiet": true}))
            .expect("a forma do que a ferramenta trazia");
        assert_eq!(shape["command"], json!(6), "o tamanho do texto em caracteres");
        assert_eq!(shape["reason"], json!(3));
        assert_eq!(shape["quiet"], json!(4), "o que não é texto conta pelo JSON dele");
        assert!(!shape.to_string().contains("ls -la"), "o conteúdo não entra: {shape}");

        assert_eq!(shape_of(&json!({})), None, "sem campo, sem forma");
        assert_eq!(shape_of(&json!("texto")), None, "o que não é objeto não tem forma");
    }

    #[test]
    fn hook_actions_are_recorded_in_the_operations_file_not_in_the_spec() {
        let dir = project_on("ganchos");
        let root = dir.path();
        let path = mustard_core::io::spec_events::spec_file(root, "ganchos").unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        record_hook(root, None, "command_guard", HookAction::Block, "Bash", "apaga trabalho").expect("hook");
        record_injection(root, None, "prompt_entry", "uma linha").expect("injection");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "the spec is left as it was");
        let observed = recorded_operations(root, "ganchos");
        let kinds: Vec<&str> = observed.events.iter().map(|e| e.event_type.as_str()).collect();
        assert_eq!(kinds, ["hook", "injection"]);
        assert_eq!((observed.events[0].str_field("action"), observed.events[0].str_field("tool")), (Some("block"), Some("Bash")));
        assert_eq!(observed.events[1].int("chars"), Some(9));
    }

    /// Toda chamada vai para o arquivo de operações: o passo do fluxo, a
    /// retomada, a procura, o pull request e a busca do mapa com o custo do
    /// Jev. Nenhuma toca na spec.
    #[test]
    fn every_call_goes_to_the_operations_file() {
        let dir = project_on("chamadas");
        let root = dir.path();
        let path = mustard_core::io::spec_events::spec_file(root, "chamadas").unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        let ok = json!({"ok": true, "spec": "chamadas"});
        for command in ["open", "resume", "round", "pr-open"] {
            assert!(record_call(root, command, Some("chamadas"), Instant::now(), &ok).is_some(), "{command}");
        }
        assert!(record_output_call(root, "find", Some("chamadas"), Instant::now(), &ok, "achado\n").is_some());
        let billed = draft(json!({"filter": "jev", "tokens": 10, "cost_micro_usd": 3}));
        assert!(record_measured_call(root, "map search", Some("chamadas"), None, Instant::now(), &ok, billed).is_some());

        assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "no call reaches the spec");
        let commands: Vec<String> =
            recorded_operations(root, "chamadas").events.iter().filter_map(|e| e.str_field("command").map(str::to_string)).collect();
        assert_eq!(commands, ["open", "resume", "round", "pr-open", "find", "map search"]);
    }

    /// A fala e a resposta só acrescentam a linha delas ao fim da spec: o que
    /// já estava no arquivo fica como estava, byte a byte, mesmo com uma linha
    /// cortada no fim, que a gravação completa reescreveria. Cada uma leva o
    /// número seguinte ao maior, contando o da linha cortada, começa numa
    /// linha nova e leva o código seguinte do tipo dela.
    #[test]
    fn the_message_and_the_answer_only_append_their_line_to_the_spec() {
        let dir = project_on("acrescenta");
        let root = dir.path();
        let path = mustard_core::io::spec_events::spec_file(root, "acrescenta").unwrap();
        let first = record_message(root, None, "primeira").expect("first message");
        let cut = first + 5;
        let before = format!("{}{{\"v\":1,\"id\":{cut},\"code\":\"MSTD-MSG-0", std::fs::read_to_string(&path).unwrap());
        std::fs::write(&path, &before).unwrap();

        let asked = record_message(root, None, "segunda").expect("message recorded");
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.starts_with(&before), "the message rewrote what was there:\n{after}");
        assert!(after[before.len()..].starts_with('\n'), "the message starts on a new line");
        assert_eq!(asked, cut + 1, "the next number counts the cut line");

        let answered = record_response(root, None, "Feito.").expect("response recorded");
        let last = std::fs::read_to_string(&path).unwrap();
        assert!(last.starts_with(&after), "the answer rewrote what was there:\n{last}");
        assert_eq!(answered, asked + 1);

        let log = DiskSpecState::new(root).log("acrescenta").unwrap();
        let message = log.get(asked).expect("the message reads");
        assert_eq!(message.str_field("code"), Some("MSTD-MSG-0002"));
        assert_eq!(log.get(answered).and_then(|answer| answer.int("reply_to")), Some(asked));
    }

    /// O que um gancho fez também só acrescenta a linha ao arquivo de
    /// operações, sem reescrever o que estava nele.
    #[test]
    fn an_observation_only_appends_its_line_to_the_operations_file() {
        let dir = project_on("operacoes");
        let root = dir.path();
        record_hook(root, None, "command_guard", HookAction::Warn, "Bash", "primeiro").expect("first hook");
        let path = mustard_core::io::spec_events::operations_file(root, "operacoes").unwrap();
        let mut before = std::fs::read_to_string(&path).unwrap();
        before.push_str("{\"v\":1,\"id\":7,\"code\":\"MSTD-HOOK-0\n");
        std::fs::write(&path, &before).unwrap();

        let id = record_hook(root, None, "command_guard", HookAction::Block, "Bash", "segundo").expect("hook recorded");
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.starts_with(&before), "the observation rewrote what was there:\n{after}");
        assert_eq!(id, 8);
        assert_eq!(recorded_operations(root, "operacoes").get(id).and_then(|e| e.str_field("code").map(str::to_string)), Some("MSTD-HOOK-0002".to_string()));
    }

    /// As chamadas medidas que saíram juntas vão ao arquivo de operações numa
    /// gravação só: a mesma hora em todas, os números seguidos, e a spec fica
    /// como estava.
    #[test]
    fn calls_measured_together_are_recorded_in_one_write() {
        let dir = project_on("lote");
        let root = dir.path();
        let path = mustard_core::io::spec_events::spec_file(root, "lote").unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        let measured: Vec<Map<String, Value>> =
            (1..=4).map(|n| draft(json!({"filter": "jev", "tokens": n * 10, "cost_micro_usd": n}))).collect();

        let ids = record_measured_calls(root, "lote", "triage", Instant::now(), measured).expect("all recorded");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "no call reaches the spec");
        let log = recorded_operations(root, "lote");
        let calls: Vec<&SpecEvent> = ids.iter().filter_map(|id| log.get(*id)).collect();
        assert_eq!(calls.len(), 4);
        assert!(ids.windows(2).all(|pair| pair[1] == pair[0] + 1), "{ids:?}");
        assert!(calls.iter().all(|call| call.at() == calls[0].at()), "one write, one time: {calls:?}");
        assert_eq!(calls.iter().filter_map(|call| call.int("tokens")).collect::<Vec<_>>(), [10, 20, 30, 40]);
    }

    /// O arquivo de operações que passa do teto manda a parte antiga ao
    /// arquivo morto ao lado: o vivo volta a ficar pequeno, os números e os
    /// códigos seguem de onde estavam, e quem lê o histórico inteiro vê cada
    /// evento uma vez, na ordem.
    #[test]
    fn an_operations_file_past_its_cap_moves_the_old_part_to_the_archive() {
        let dir = project_on("teto");
        let root = dir.path();
        let text = "x".repeat(1000);
        let injection = || draft(json!({"author": "hook", "hook": "prompt_entry", "chars": 1000, "text": text}));
        record_hook(root, None, "command_guard", HookAction::Warn, "Bash", "antigo").expect("hook");
        for _ in 0..12 {
            observe(root, "teto", "injection", (0..100).map(|_| injection()).collect()).expect("recorded");
        }
        let live = mustard_core::io::spec_events::operations_file(root, "teto").unwrap();
        let size = std::fs::metadata(&live).unwrap().len();
        assert!(size < 512 * 1024, "the live file kept growing: {size} bytes");

        let next = observe(root, "teto", "injection", vec![injection()]).expect("recorded")[0];
        assert_eq!(next, 1202, "the numbers go on from where they were");
        let hook = record_hook(root, None, "command_guard", HookAction::Block, "Bash", "novo").expect("hook");
        let log = recorded_operations(root, "teto");
        let ids: Vec<u64> = log.events.iter().map(|e| e.id).collect();
        assert_eq!(ids, (1..=1203).collect::<Vec<u64>>(), "every event once, in order");
        assert_eq!(log.get(next).and_then(|e| e.str_field("code")), Some("MSTD-INJ-1201"));
        assert_eq!(log.get(hook).and_then(|e| e.str_field("code")), Some("MSTD-HOOK-0002"));
    }

    /// Sem spec atual nada é gravado, nem a mensagem nem a resposta; e a
    /// mensagem em branco também não.
    #[test]
    fn nothing_is_recorded_without_a_live_spec() {
        let bare = tempfile::tempdir().expect("temp dir");
        assert_eq!(record_message(bare.path(), None, "oi"), None);
        assert_eq!(record_response(bare.path(), None, "resposta solta"), None);
        assert!(!bare.path().join(".claude").exists(), "no spec folder is created");

        let dir = project_on("sem-pergunta");
        assert_eq!(record_message(dir.path(), None, "   "), None);
    }

    fn event_lines(root: &Path, spec: &str) -> usize {
        let path = mustard_core::io::spec_events::spec_file(root, spec).expect("spec file");
        std::fs::read_to_string(path).expect("events").lines().count()
    }

    /// A resposta gravada por dentro do binário sem `reply_to`, como
    /// [`record_response`] a grava quando não acha mensagem do usuário.
    fn loose_response(root: &Path, spec: &str, text: &str) -> Result<u64, mustard_core::domain::spec_events::Refusal> {
        let fields = draft(json!({ "author": "assistant", "text": text }));
        super::super::write::record(root, spec, "response", fields, PhaseWriter::Binary).map(|r| r.written.id)
    }

    /// A resposta do turno em que a spec nasce, antes de qualquer mensagem do
    /// usuário, é gravada sem `reply_to`; depois da mensagem, a resposta
    /// aponta a mensagem. A resposta sem `reply_to` numa spec que já tem
    /// mensagem do usuário é recusada pela falta do campo, e nada é gravado.
    #[test]
    fn only_the_answer_before_any_message_goes_without_reply_to() {
        let dir = project_on("abertura");
        let root = dir.path();
        record_response(root, None, "Sugiro um objetivo.").expect("the opening answer is recorded");
        let asked = record_message(root, None, "pode usar essa").expect("message");
        record_response(root, None, "Gravei.").expect("response");
        let responses = events_of(root, "abertura", "response");
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0].get("reply_to"), None);
        assert_eq!(responses[1]["reply_to"], json!(asked));

        let before = event_lines(root, "abertura");
        let refused = loose_response(root, "abertura", "Solta.").err();
        let missing = mustard_core::domain::spec_events::Refusal::MissingField { event_type: "response".to_string(), field: "reply_to".to_string() };
        assert_eq!(refused, Some(missing));
        assert_eq!(event_lines(root, "abertura"), before, "a refusal writes nothing");
    }

    /// A mensagem do usuário e a resposta do fim do turno gravadas ao mesmo
    /// tempo, pelos dois ganchos: a resposta decide o `reply_to` antes de
    /// pegar a trava, e a conferência roda com a trava presa, então uma
    /// resposta sem `reply_to` nunca fica depois de uma mensagem do usuário.
    #[test]
    fn a_message_and_the_end_of_an_answer_at_the_same_time() {
        for round in 0..40 {
            let dir = project_on("junto");
            let root = dir.path();
            let gate = std::sync::Barrier::new(2);
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    gate.wait();
                    record_message(root, None, "pode usar essa")
                });
                scope.spawn(|| {
                    gate.wait();
                    record_response(root, None, "Sugiro um objetivo.")
                });
            });
            let log = DiskSpecState::new(root).log("junto").expect("log");
            let asked = log.visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id);
            let asked = asked.expect("the message is always recorded");
            for answer in log.visible().into_iter().filter(|e| e.event_type == "response") {
                let replied = answer.int("reply_to");
                assert!(
                    replied == Some(asked) || (replied.is_none() && answer.id < asked),
                    "round {round}: response {} with reply_to {replied:?} and the message {asked}",
                    answer.id
                );
            }
        }
    }

    /// Uma chave inventada no formato de uma chave de verdade: `apikey_`, 36
    /// letras e números em hexadecimal, `_` e mais 64. Devolve a chave e os
    /// dois pedaços dela.
    fn pasted_key() -> (String, String, String) {
        let (head, tail) = ("0a1b2c3d4e5f".repeat(3), "9f8e7d6c5b4a3210".repeat(4));
        (format!("apikey_{head}_{tail}"), head, tail)
    }

    /// Um evento do Claude Code na sessão `s1` do projeto em `root`, pelo
    /// despachante inteiro, como o `mustard-rt on <evento>` o roda.
    fn hook_event(root: &Path, name: &str, raw: Value) {
        use mustard_core::domain::model::contract::{HookInput, Trigger};
        let input = HookInput {
            hook_event_name: Some(name.to_string()),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            raw,
            ..HookInput::default()
        };
        let _ = crate::dispatch::run_event(Trigger::from_event_name(name), &input);
    }

    /// As linhas do arquivo de eventos da spec, como estão no disco: o campo
    /// de busca vai junto.
    fn file_lines(root: &Path, spec: &str) -> Vec<String> {
        let path = mustard_core::io::spec_events::spec_file(root, spec).expect("spec file");
        std::fs::read_to_string(path).expect("events").lines().map(str::to_string).collect()
    }

    /// A chave colada sozinha na mensagem, pelo gancho da mensagem, e a
    /// resposta que a repete, pelo gancho do fim do turno, são gravadas com
    /// "…" no lugar da chave. Nenhum pedaço dela fica em campo nenhum da
    /// linha, nem no de busca.
    #[test]
    fn a_pasted_key_is_recorded_as_the_mark_in_the_message_and_the_answer() {
        let dir = project_on("chave");
        let root = dir.path();
        let (key, head, tail) = pasted_key();
        hook_event(root, "UserPromptSubmit", json!({ "prompt": key }));
        hook_event(root, "Stop", json!({ "last_assistant_message": format!("Recebi a chave {key}; não a guarde.") }));

        let message = &events_of(root, "chave", "message")[0];
        assert_eq!(message["text"], json!("apikey_…"));
        let response = &events_of(root, "chave", "response")[0];
        assert_eq!(response["text"], json!("Recebi a chave apikey_…; não a guarde."));
        let lines = file_lines(root, "chave");
        let spoken: Vec<&String> = lines.iter().filter(|line| line.contains(r#""type":"message""#) || line.contains(r#""type":"response""#)).collect();
        assert_eq!(spoken.len(), 2, "{lines:?}");
        for line in lines {
            assert!(!line.contains(&head) && !line.contains(&tail), "a piece of the key reached the file: {line}");
        }
    }

    /// A página da spec, preparada depois da conversa com a chave colada,
    /// leva a mensagem com a marca no lugar da chave, e a chave em lugar
    /// nenhum dos arquivos que vão para o banco dela.
    #[test]
    fn the_spec_page_never_carries_a_pasted_key() {
        let dir = project_on("pagina");
        let root = dir.path();
        let (key, head, tail) = pasted_key();
        hook_event(root, "UserPromptSubmit", json!({"prompt":format!("use esta: {key}")}));
        let projection = crate::commands::panel::snapshot(root, Some("pagina")).to_string();
        assert!(!projection.contains(&head) && !projection.contains(&tail));
        assert!(!root.join(".claude/mustard/publications").exists());
        let export = crate::commands::panel::prepare_publication(root, "pagina", false);
        assert_eq!(export["ok"], true, "{export}");
        for field in ["database", "page", "manifest"] {
            let body = std::fs::read_to_string(export[field].as_str().unwrap()).unwrap();
            assert!(!body.contains(&head) && !body.contains(&tail), "{field}: {body}");
        }
    }

    /// O hash de um commit, curto ou inteiro, o código de um item e o
    /// caminho de um arquivo continuam gravados como o usuário os escreveu.
    #[test]
    fn a_commit_hash_an_item_code_and_a_path_are_recorded_as_written() {
        let dir = project_on("comum");
        let root = dir.path();
        let text = "o commit 13b27dc3 e o 13b27dc3a4f5e6d7c8b9a0f1e2d3c4b5a6f7e8d9 fecharam a MSTD-TASK-0092 \
                    em apps/rt/src/shared/secret.rs:41";
        hook_event(root, "UserPromptSubmit", json!({ "prompt": text }));
        hook_event(root, "Stop", json!({ "last_assistant_message": text }));
        assert_eq!(events_of(root, "comum", "message")[0]["text"], json!(text));
        assert_eq!(events_of(root, "comum", "response")[0]["text"], json!(text));
    }

    /// A resposta a uma pergunta com as opções da mudança, pelo despachante
    /// inteiro, como o gancho do fim de uma ferramenta a roda: o menu `options`,
    /// o clique em `option`, a nota `notes` escrita ao lado e o código da
    /// mudança no cabeçalho.
    fn answer_choice(root: &Path, question: &str, code: &str, options: &[&str], option: &str, notes: &str) {
        use mustard_core::domain::model::contract::{HookInput, Trigger};
        let options: Vec<Value> = options.iter().map(|label| json!({ "label": label })).collect();
        let input = HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("AskUserQuestion".to_string()),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            tool_input: json!({ "questions": [{ "question": question, "header": code, "options": options }] }),
            raw: json!({ "tool_response": {
                "answers": { question: option },
                "annotations": { question: { "notes": notes } },
            } }),
            ..HookInput::default()
        };
        let _ = crate::dispatch::run_event(Some(Trigger::PostToolUse), &input);
    }

    /// Uma senha e um token do GitHub inventados, montados aqui de pedaços.
    fn typed_secrets() -> (String, String) {
        (["S3nh4", "F0rte", "2024"].concat(), format!("ghp_{}", "a1".repeat(18)))
    }

    /// A nota digitada ao lado da opção clicada numa pergunta da mudança,
    /// com uma senha escrita como atribuição e um token do GitHub, é gravada
    /// com "…" no lugar de cada um, e nenhum dos dois fica em campo nenhum da
    /// linha. A testemunha continua com a pergunta, a opção e o código da
    /// mudança como vieram.
    #[test]
    fn a_note_typed_beside_the_clicked_option_is_recorded_without_the_secret() {
        let dir = project_on("escolha");
        let root = dir.path();
        let (password, token) = typed_secrets();
        let question = "A onda 2 precisa da 1 antes. Posso mudar o plano?";
        let code = "onda-2-abc123";
        let notes = format!("a senha: {password} e o token {token}");
        answer_choice(root, question, code, &["Aceitar", "Recusar"], "Aceitar", &notes);

        let message = &events_of(root, "escolha", "message")[0];
        assert_eq!(message["text"], json!("Aceitar\na senha: … e o token …"));
        assert_eq!(message["witness"], json!({ "question": question, "answer": "Aceitar", "change": code }));
        for line in file_lines(root, "escolha") {
            assert!(!line.contains(&password) && !line.contains(&token), "a secret reached the file: {line}");
        }
    }

    /// A pergunta que repete o token e a opção clicada que o traz vão para a
    /// testemunha sem ele; o código da mudança fica igual.
    #[test]
    fn the_witness_question_and_option_that_carry_a_secret_are_recorded_without_it() {
        let dir = project_on("opcao");
        let root = dir.path();
        let (_, token) = typed_secrets();
        let question = format!("Uso o token {token} na onda 2?");
        let option = format!("Usar o token {token}");
        let code = "onda-2-abc123";
        answer_choice(root, &question, code, &["Aceitar", "Recusar", &option], &option, "");

        let message = &events_of(root, "opcao", "message")[0];
        assert_eq!(message["text"], json!("Usar o token …"));
        let witness = json!({ "question": "Uso o token … na onda 2?", "answer": "Usar o token …", "change": code });
        assert_eq!(message["witness"], witness);
        for line in file_lines(root, "opcao") {
            assert!(!line.contains(&token), "the token reached the file: {line}");
        }
    }

    /// A chamada de um passo grava o comando, o resultado e, na recusa, a
    /// razão; no arquivo de operações da spec do relatório.
    #[test]
    fn a_call_records_its_result() {
        let dir = project_on("chamada");
        let root = dir.path();
        let started = Instant::now();
        record_call(root, "plan", None, started, &json!({"ok": true, "spec": "chamada"})).expect("ok call");
        record_call(root, "round", Some("chamada"), started, &json!({"ok": false, "reason": "not-approved"})).expect("refused call");
        let calls: Vec<Map<String, Value>> = recorded_operations(root, "chamada").events.into_iter().map(|e| e.fields).collect();
        assert_eq!(calls.len(), 2);
        assert_eq!((calls[0]["command"].as_str(), calls[0]["result"].as_str()), (Some("plan"), Some("ok")));
        assert!(calls[0].get("refusal").is_none());
        assert_eq!(calls[1]["result"], json!("refused"));
        assert_eq!(calls[1]["refusal"], json!("not-approved"));

        // Uma spec que não existe não ganha arquivo.
        assert_eq!(record_call(root, "plan", Some("outra"), started, &json!({"ok": true})), None);
        assert!(mustard_core::io::spec_events::operations_file(root, "outra").is_some_and(|path| !path.exists()));
    }
}
