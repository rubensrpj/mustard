//! A conversa gravada no bloco da spec atual: a mensagem do usuário, a
//! resposta do assistente, o que um gancho fez e cada chamada de um passo do
//! fluxo.
//!
//! Quem grava é o binário, nunca o modelo: a mensagem e a resposta chegam
//! pelo Claude Code, e o despachante sabe o que cada gancho decidiu. Tudo
//! passa pela mesma gravação do `run write`, com o autor de cada um.
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

use mustard_core::domain::spec_events::{SpecEvent, SpecLog};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState, State, last_user_message};
use mustard_core::domain::survey;
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

/// A spec que recebe a conversa de `session` no projeto em `root`: a spec
/// atual, pela escada única, quando ela tem arquivo de eventos e não
/// terminou.
pub(crate) fn conversation_spec(root: &Path, session: Option<&str>) -> Option<String> {
    let disk = DiskSpecState::new(root);
    let spec = disk.active(session)?;
    let log = disk.log(&spec)?;
    let phase = State::from_log(&log).phase;
    (!phase.is_some_and(|phase| FINISHED.contains(&phase))).then_some(spec)
}

/// Grava um evento na spec da conversa; o número dele, ou `None` quando não
/// há spec ou a gravação foi recusada.
fn record(root: &Path, session: Option<&str>, event_type: &str, draft: Map<String, Value>) -> Option<u64> {
    let spec = conversation_spec(root, session)?;
    record_in_spec(root, &spec, event_type, draft)
}

/// Grava um evento na spec `spec`, pela gravação do `run write`.
fn record_in_spec(root: &Path, spec: &str, event_type: &str, draft: Map<String, Value>) -> Option<u64> {
    super::write::record(root, spec, event_type, draft, PhaseWriter::Binary).ok().map(|recorded| recorded.written.id)
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
    record_in_spec(root, &spec, "message", draft(fields))
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
/// mensagens seguintes ficam com o gancho. Devolve os números gravados.
pub(crate) fn record_earlier_messages(root: &Path, spec: &str, texts: &[String]) -> Vec<u64> {
    texts
        .iter()
        .filter(|text| !text.trim().is_empty())
        .filter_map(|text| {
            record_in_spec(root, spec, "message", draft(json!({ "author": "user", "text": without_secrets(text) })))
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
    if let Some(asked) = last_user_message(&log) {
        fields["reply_to"] = json!(asked.id);
    }
    record_in_spec(root, &spec, "response", draft(fields))
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
    record(root, session, "hook", draft(json!({ "author": "hook", "hook": hook, "action": action.name(), "tool": tool, "reason": reason })))
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
    if ["knowledge","expertise"].contains(&command) {
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
        ("search_selection_cached","/task_context/selection/cached"),("search_accepted_choices","/task_context/selection/accepted_choices")] {
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
/// A chamada que usou o Jev e não ficou numa spec, por não haver uma ou pela
/// gravação recusada, deixa os tokens e o custo dele no arquivo das chamadas
/// soltas da máquina. Esses registros não representam saldo do provedor.
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
    let tokens = measured.get("tokens").and_then(Value::as_u64);
    let cost = measured.get("cost_micro_usd").and_then(Value::as_u64);
    let recorded = record_in_call_spec(root, command, named, session, started, report, measured);
    let billed = tokens.is_some() || cost.is_some();
    if let (None, true, Some(dir)) = (recorded, billed, spend::machine_dir()) {
        let _ = spend::record_loose_call(&dir, root, tokens.unwrap_or(0), cost.unwrap_or(0));
    }
    recorded
}

/// Transport observations must not consume item IDs, change a refused
/// write, or masquerade as an agent's required reading receipt.
fn operations_path(root:&Path,spec:&str)->Option<std::path::PathBuf> {
    let canonical=mustard_core::io::spec_events::spec_file(root,spec).ok()?;
    let name=canonical.parent()?.file_name()?;
    Some(mustard_core::io::spec_events::spec_root(root).join(".claude/mustard/operations").join(name).with_extension("ndjson"))
}

pub(crate) fn recorded_operations(root:&Path,spec:&str)->mustard_core::domain::spec_events::SpecLog {
    operations_path(root,spec).and_then(|path|mustard_core::io::spec_events::read(&path).ok().flatten()).unwrap_or_default()
}

fn project_operations_path(root:&Path)->std::path::PathBuf {
    mustard_core::io::spec_events::spec_root(root).join(".claude/mustard/operations/.project.ndjson")
}

pub(crate) fn project_operations(root:&Path)->mustard_core::domain::spec_events::SpecLog {
    mustard_core::io::spec_events::read(&project_operations_path(root)).ok().flatten().unwrap_or_default()
}

/// A gravação de [`record_measured_call`] na spec do relatório, na que a
/// chamada nomeou ou na atual; `None` sem nenhuma delas ou com a gravação
/// recusada.
fn record_in_call_spec(
    root: &Path,
    command: &str,
    named: Option<&str>,
    session: Option<&str>,
    started: Instant,
    report: &Value,
    measured: Map<String, Value>,
) -> Option<u64> {
    let spec = report
        .get("spec")
        .and_then(Value::as_str)
        .or(named)
        .map(str::trim)
        .filter(|spec| !spec.is_empty())
        .map(str::to_string)
        .or_else(|| conversation_spec(root, session));
    if let Some(spec) = &spec { DiskSpecState::new(root).log(spec)?; }
    else if command != "knowledge" { return None; }
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
    let mut fields = draft(fields);
    fields.extend(measured);
    let Some(spec) = spec else {
        return mustard_core::io::spec_events::write(&project_operations_path(root),"call",fields,&[]).ok().map(|event|event.id);
    };
    let argument_failure = !ok && matches!(reason,Some("duplicate-option"|"invalid-arguments"));
    if (fields.get("invocation")==Some(&json!(true)) && matches!(command,"read"|"write"|"search"|"knowledge"|"expertise")) || argument_failure {
        let path=operations_path(root,&spec)?;
        return mustard_core::io::spec_events::write(&path,"call",fields,&[]).ok().map(|recorded|recorded.id);
    }
    record_in_spec(root, &spec, "call", fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::record_open;
    use crate::shared::spec_state::stand_on_spec_branch;

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
        let receipt=draft(json!({"request":"request-1","item":"MSTD-RULE-0001"}));
        assert!(record_measured_call(root,"read",Some("medicao"),None,Instant::now(),&json!({"ok":true}),receipt).is_some());
        let receipts=events_of(root,"medicao","call");
        assert_eq!(receipts.len(),1);
        assert_eq!(receipts[0]["request"],"request-1");
        assert_eq!(recorded_operations(root,"medicao").events.len(),8);
        assert!(record_call(root,"open",Some("medicao"),Instant::now(),&json!({"ok":true,"kind":"feature"})).is_some());
        assert_eq!(events_of(root,"medicao","call").len(),2,"the domain's work kind is not a CLI parse error");
        assert_eq!(recorded_operations(root,"medicao").events.len(),8);
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
        dir
    }

    fn events_of(root: &Path, spec: &str, event_type: &str) -> Vec<Map<String, Value>> {
        DiskSpecState::new(root)
            .log(spec)
            .map(|log| log.visible().into_iter().filter(|e| e.event_type == event_type).map(|e| e.fields.clone()).collect())
            .unwrap_or_default()
    }

    /// Com spec atual, a mensagem, a resposta ligada a ela, a decisão de um
    /// gancho e o texto colocado vão para o bloco da conversa, cada um com o
    /// autor certo.
    #[test]
    fn the_conversation_lands_in_the_current_spec() {
        let dir = project_on("conversa");
        let root = dir.path();
        let asked = record_message(root, None, "arrume o login").expect("message recorded");
        record_response(root, None, "Arrumei.").expect("response recorded");
        record_hook(root, None, "command_guard", HookAction::Block, "Bash", "apaga trabalho").expect("hook");
        record_injection(root, None, "prompt_entry", "uma linha").expect("injection");

        let message = &events_of(root, "conversa", "message")[0];
        assert_eq!((message["author"].as_str(), message["text"].as_str()), (Some("user"), Some("arrume o login")));
        let response = &events_of(root, "conversa", "response")[0];
        assert_eq!(response["reply_to"], json!(asked));
        assert_eq!(response["author"], json!("assistant"));
        let hook = &events_of(root, "conversa", "hook")[0];
        assert_eq!((hook["action"].as_str(), hook["tool"].as_str()), (Some("block"), Some("Bash")));
        let injection = &events_of(root, "conversa", "injection")[0];
        assert_eq!(injection["chars"], json!(9));
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
        assert_eq!(message["text"], json!(format!("{question}\nAceitar\na senha: … e o token …")));
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
        assert_eq!(message["text"], json!("Uso o token … na onda 2?\nUsar o token …"));
        let witness = json!({ "question": "Uso o token … na onda 2?", "answer": "Usar o token …", "change": code });
        assert_eq!(message["witness"], witness);
        for line in file_lines(root, "opcao") {
            assert!(!line.contains(&token), "the token reached the file: {line}");
        }
    }

    /// A chamada de um passo grava o comando, o resultado e, na recusa, a
    /// razão; na spec do relatório.
    #[test]
    fn a_call_records_its_result() {
        let dir = project_on("chamada");
        let root = dir.path();
        let started = Instant::now();
        record_call(root, "plan", None, started, &json!({"ok": true, "spec": "chamada"})).expect("ok call");
        record_call(root, "round", Some("chamada"), started, &json!({"ok": false, "reason": "not-approved"})).expect("refused call");
        let calls = events_of(root, "chamada", "call");
        assert_eq!(calls.len(), 2);
        assert_eq!((calls[0]["command"].as_str(), calls[0]["result"].as_str()), (Some("plan"), Some("ok")));
        assert!(calls[0].get("refusal").is_none());
        assert_eq!(calls[1]["result"], json!("refused"));
        assert_eq!(calls[1]["refusal"], json!("not-approved"));

        // Uma spec que não existe não ganha arquivo.
        assert_eq!(record_call(root, "plan", Some("outra"), started, &json!({"ok": true})), None);
    }
}
