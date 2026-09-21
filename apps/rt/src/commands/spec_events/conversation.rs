//! A conversa gravada no bloco da spec atual: a mensagem do usuário, a
//! resposta do assistente, o que um gancho fez e cada chamada de um passo do
//! fluxo.
//!
//! Quem grava é o binário, nunca o modelo: a mensagem e a resposta chegam
//! pelo Claude Code, e o despachante sabe o que cada gancho decidiu. Tudo
//! passa pela mesma gravação do `run write`, com o autor de cada um.
//!
//! Só se grava numa spec atual, que tem arquivo de eventos e ainda não
//! terminou: sem ela, nada é gravado. Nenhuma gravação daqui falha para quem
//! chama — uma recusa ou um erro de disco vira "nada gravado".

use std::path::Path;
use std::time::Instant;

use mustard_core::domain::spec_state::{last_user_message, PhaseWriter, SpecState, State};
use serde_json::{json, Map, Value};

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
    super::write::record(root, spec, event_type, draft, PhaseWriter::Binary)
        .ok()
        .map(|recorded| recorded.written.id)
}

fn draft(fields: Value) -> Map<String, Value> {
    match fields {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// A mensagem do usuário, como ele a escreveu.
pub(crate) fn record_message(root: &Path, session: Option<&str>, text: &str) -> Option<u64> {
    if text.trim().is_empty() {
        return None;
    }
    record(root, session, "message", draft(json!({ "author": "user", "text": text })))
}

/// A resposta do usuário a uma pergunta de gesto, com a testemunha: a
/// pergunta e a opção que ele clicou. É o registro que o fluxo lê como o
/// "sim" do usuário, e só a testemunha o grava.
pub(crate) fn record_witnessed_message(
    root: &Path,
    session: Option<&str>,
    text: &str,
    question: &str,
    answer: &str,
) -> Option<u64> {
    if text.trim().is_empty() {
        return None;
    }
    let fields = json!({
        "author": "user",
        "text": text,
        "witness": { "question": question, "answer": answer },
    });
    record(root, session, "message", draft(fields))
}

/// A resposta do assistente ao fim do turno, ligada à última mensagem do
/// usuário. No turno em que a spec nasce ainda não há mensagem do usuário
/// gravada nela: a resposta vai sem `reply_to`, e o sim à sugestão feita ali
/// acha a resposta que respondeu. A resposta que a regra das pendências
/// barrou também passa por aqui, e o complemento que o bloqueio pediu vem
/// depois dela, ligado à mesma mensagem.
pub(crate) fn record_response(root: &Path, session: Option<&str>, text: &str) -> Option<u64> {
    if text.trim().is_empty() {
        return None;
    }
    let spec = conversation_spec(root, session)?;
    let log = DiskSpecState::new(root).log(&spec)?;
    let mut fields = json!({ "author": "assistant", "text": text });
    if let Some(asked) = last_user_message(&log) {
        fields["reply_to"] = json!(asked.id);
    }
    record_in_spec(root, &spec, "response", draft(fields))
}

/// Um gancho barrou ou avisou: quem, o quê, em que ferramenta (ou evento) e
/// por quê.
pub(crate) fn record_hook(
    root: &Path,
    session: Option<&str>,
    hook: &str,
    action: HookAction,
    tool: &str,
    reason: &str,
) -> Option<u64> {
    record(
        root,
        session,
        "hook",
        draft(json!({ "author": "hook", "hook": hook, "action": action.name(), "tool": tool, "reason": reason })),
    )
}

/// Um gancho colocou texto na conversa: quem, o texto e o tamanho dele em
/// caracteres.
pub(crate) fn record_injection(root: &Path, session: Option<&str>, hook: &str, text: &str) -> Option<u64> {
    record(
        root,
        session,
        "injection",
        draft(json!({ "author": "hook", "hook": hook, "chars": text.chars().count(), "text": text })),
    )
}

/// Uma chamada de um passo do fluxo: o comando, quanto tempo levou, se deu
/// certo, e a razão da recusa (`reason`, ou o `error` das portas do pull
/// request). A spec é a do relatório, a que a chamada nomeou, ou a atual.
pub(crate) fn record_call(root: &Path, command: &str, named: Option<&str>, started: Instant, report: &Value) -> Option<u64> {
    let spec = report
        .get("spec")
        .and_then(Value::as_str)
        .or(named)
        .map(str::trim)
        .filter(|spec| !spec.is_empty())
        .map(str::to_string)
        .or_else(|| conversation_spec(root, crate::shared::spec_state::session_from_env().as_deref()))?;
    DiskSpecState::new(root).log(&spec)?;
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
    record_in_spec(root, &spec, "call", draft(fields))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::record_open;
    use crate::shared::spec_state::stand_on_spec_branch;

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
            .map(|log| {
                log.visible()
                    .into_iter()
                    .filter(|e| e.event_type == event_type)
                    .map(|e| e.fields.clone())
                    .collect()
            })
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
        let missing = mustard_core::domain::spec_events::Refusal::MissingField {
            event_type: "response".to_string(),
            field: "reply_to".to_string(),
        };
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

    /// A chamada de um passo grava o comando, o resultado e, na recusa, a
    /// razão; na spec do relatório.
    #[test]
    fn a_call_records_its_result() {
        let dir = project_on("chamada");
        let root = dir.path();
        let started = Instant::now();
        record_call(root, "plan", None, started, &json!({"ok": true, "spec": "chamada"})).expect("ok call");
        record_call(root, "round", Some("chamada"), started, &json!({"ok": false, "reason": "not-approved"}))
            .expect("refused call");
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
