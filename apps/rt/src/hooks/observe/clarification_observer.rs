//! `clarification_observer` — PostToolUse(AskUserQuestion): grava cada pergunta
//! respondida como esclarecimento da unidade ativa.
//!
//! ## Por que existe
//!
//! O documento que o usuário lê antes de aprovar uma spec mostra o que foi
//! esclarecido — a pergunta e a resposta. O binário não guarda as falas do
//! assistente, mas a resposta a uma pergunta chega inteira no
//! `tool_response.answers` do PostToolUse, e ali ela pode ser capturada sem
//! depender de o assistente lembrar de registrá-la. Disciplina que depende de
//! memória é a que falha; captura determinística não.
//!
//! ## Os fatos que precisam valer TODOS
//!
//! 1. **Uma resposta de verdade.** `tool_response.answers` traz ao menos uma
//!    pergunta com resposta não vazia. Um diálogo cancelado (`{}`) não
//!    esclareceu nada. É o teste mais barato (puro, sem IO), então roda
//!    primeiro: este observador vê TODA pergunta da sessão.
//! 2. **Uma unidade ativa.** A sessão está ligada a uma spec
//!    (`spec_for_session`) ou, sem esse vínculo, há uma spec ativa
//!    (`current_spec`), e ela não está concluída (`meta.json`). O diretório
//!    dela precisa existir — o `material_add::add` recusa um slug sem
//!    diretório, e essa recusa é o "sem unidade, nada é gravado".
//!
//! ## Por que texto livre TAMBÉM vale aqui
//!
//! O `approval_marker_observer` recusa resposta digitada porque ela destrava um
//! portão. Este observador não destrava nada: registra o que o usuário disse.
//! A resposta vem do harness, não do modelo — é isso que a torna confiável
//! como registro —, e o que o usuário escreveu pelo `Other` é exatamente o tipo
//! de esclarecimento que o documento precisa mostrar.
//!
//! ## A escrita
//!
//! Pela MESMA porta do assistente: `material_add::add` com
//! `kind = clarification`. Dedupe, recusa de arquivo corrompido e forma do JSON
//! são os mesmos, então não existe um segundo formato para divergir do leitor.
//! Fail-open: qualquer recusa ou erro de IO vira nada, e nada é impresso — a
//! saída padrão de um hook é o canal do JSON do harness.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use std::path::Path;

use crate::commands::spec::material_add::{add, MaterialAddOpts};
use crate::shared::context::{current_spec, spec_for_session};

/// O gravador de esclarecimentos do PostToolUse(AskUserQuestion).
pub struct ClarificationObserver;

/// Uma pergunta respondida, já na forma que o material guarda.
#[derive(Debug, PartialEq, Eq)]
struct Answered {
    question: String,
    answer: String,
    notes: Option<String>,
}

/// Fato 1 — cada pergunta com resposta não vazia em `tool_response.answers`
/// (`{<pergunta>: <rótulo> | [<rótulo>, …]}`). Várias escolhas viram uma
/// resposta só, separada por vírgula. As notas vêm de
/// `tool_response.annotations.<pergunta>.notes`, quando o usuário escreveu
/// alguma; lidas com folga, porque o harness acrescenta campos com o tempo.
fn answered_questions(input: &HookInput) -> Vec<Answered> {
    input
        .ask_answers()
        .items
        .into_iter()
        .filter_map(|item| {
            let question = item.question.trim();
            let answer = item.labels.iter().map(|l| l.trim()).collect::<Vec<_>>().join(", ");
            if question.is_empty() || answer.is_empty() {
                return None;
            }
            let notes = item
                .notes
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_string);
            Some(Answered { question: question.to_string(), answer, notes })
        })
        .collect()
}

/// Has this unit already reached a terminal outcome?
///
/// Read from `meta.json`, the single lifecycle source. Fail-open: an absent or
/// unreadable sidecar answers "not closed", so the gate still applies to a unit
/// whose state cannot be read — the direction that keeps the reminder working
/// rather than silently disabling it.
pub(crate) fn spec_is_closed(root: &Path, spec: &str) -> bool {
    let spec_md = root.join(".claude").join("spec").join(spec).join("spec.md");
    mustard_core::domain::meta::read_meta_beside(&spec_md)
        .and_then(|m| m.outcome)
        .and_then(|o| mustard_core::Outcome::parse(&o))
        .is_some_and(|o| o == mustard_core::Outcome::Completed)
}

/// Fato 2 — a unidade em que o esclarecimento mora: a ligação da sessão
/// primeiro (precisa), a spec ativa depois. Uma unidade já concluída não
/// recebe material novo. `None` em qualquer dúvida.
fn active_unit(cwd: &str, input: &HookInput) -> Option<String> {
    let sid = input.session_id.as_deref().unwrap_or("");
    let spec = spec_for_session(cwd, sid).or_else(|| current_spec(cwd))?;
    let spec = spec.trim();
    if spec.is_empty() || spec_is_closed(Path::new(cwd), spec) {
        return None;
    }
    Some(spec.to_string())
}

impl Observer for ClarificationObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let answered = answered_questions(input);
        if answered.is_empty() {
            return;
        }
        let cwd = ctx.project_dir_or_cwd(input);
        let Some(spec) = active_unit(&cwd, input) else {
            return;
        };
        let root = Path::new(&cwd);
        for item in answered {
            // Best-effort: uma recusa (spec sem diretório, material corrompido)
            // não grava nada e não interrompe a sessão.
            let _ = add(
                root,
                &MaterialAddOpts {
                    spec: spec.clone(),
                    kind: "clarification".to_string(),
                    subject: item.question,
                    detail: item.answer,
                    line: None,
                    severity: None,
                    notes: item.notes,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec::material_add::MATERIAL_FILE;
    use mustard_core::domain::model::contract::Trigger;
    use serde_json::{json, Value};
    use tempfile::tempdir;

    fn ctx(dir: &str) -> Ctx {
        Ctx::for_test(dir.to_string(), Some(Trigger::PostToolUse))
    }

    /// O PostToolUse(AskUserQuestion) na forma que o harness entrega: o menu
    /// em `tool_input`, a resposta e as notas em `tool_response`.
    fn ask_input(session: &str, response: Value) -> HookInput {
        HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("AskUserQuestion".to_string()),
            session_id: Some(session.to_string()),
            tool_input: json!({
                "questions": [{ "question": "Abrir o navegador sozinho?", "header": "Entrega",
                                "options": [{ "label": "Só na aprovação" }, { "label": "Sempre" }] }]
            }),
            raw: json!({ "tool_response": response }),
            ..HookInput::default()
        }
    }

    fn seed_unit(root: &Path, spec: &str, outcome: &str) {
        let dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("meta.json"),
            format!(r#"{{"scope":"full","stage":"Plan","outcome":"{outcome}"}}"#),
        )
        .unwrap();
    }

    fn bind_session(root: &Path, session: &str, spec: &str) {
        let d = root.join(".claude").join(".session").join(session);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("active-spec"), spec).unwrap();
    }

    fn material(root: &Path, spec: &str) -> Option<Value> {
        let raw = std::fs::read_to_string(
            root.join(".claude").join("spec").join(spec).join(MATERIAL_FILE),
        )
        .ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// A pergunta respondida vira esclarecimento da unidade ativa, com
    /// a resposta escolhida e as notas do usuário, sem o assistente fazer nada.
    #[test]
    fn answered_question_is_recorded_as_clarification() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_unit(root, "demo", "Active");
        bind_session(root, "s-1", "demo");
        let input = ask_input(
            "s-1",
            json!({
                "questions": [],
                "answers": {
                    "Abrir o navegador sozinho?": "Só na aprovação",
                    "Quais documentos?": ["spec", "qa-report"]
                },
                "annotations": {
                    "Abrir o navegador sozinho?": { "notes": "uma vez por versão" }
                }
            }),
        );
        ClarificationObserver.observe(&input, &ctx(root.to_str().unwrap()));

        let doc = material(root, "demo").expect("the answer must be recorded");
        let items = doc["clarifications"].as_array().expect("clarifications array");
        assert_eq!(items.len(), 2, "one clarification per answered question: {doc}");
        let browser = items
            .iter()
            .find(|c| c["question"] == "Abrir o navegador sozinho?")
            .expect("the question is the key the harness answered");
        assert_eq!(browser["answer"], "Só na aprovação");
        assert_eq!(browser["notes"], "uma vez por versão");
        let docs = items.iter().find(|c| c["question"] == "Quais documentos?").unwrap();
        assert_eq!(docs["answer"], "spec, qa-report", "a multi-select joins its labels");
        assert!(docs.get("notes").is_none(), "no notes, no key");

        // A mesma resposta de novo não duplica o registro.
        ClarificationObserver.observe(&input, &ctx(root.to_str().unwrap()));
        let again = material(root, "demo").unwrap();
        assert_eq!(again["clarifications"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn a_dismissed_dialog_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_unit(root, "demo", "Active");
        bind_session(root, "s-1", "demo");
        let input = ask_input("s-1", json!({ "questions": [], "answers": {} }));
        ClarificationObserver.observe(&input, &ctx(root.to_str().unwrap()));
        assert!(material(root, "demo").is_none(), "a cancelled question clarified nothing");
    }

    #[test]
    fn without_an_active_unit_nothing_is_recorded() {
        // Uma spec ativa herdada do ambiente mudaria a resolução; pular é
        // melhor que um teste que depende do shell de quem roda.
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        // A unidade existe, mas a sessão não está ligada a ela e nada a aponta
        // como ativa.
        seed_unit(root, "demo", "Active");
        let input = ask_input("s-free", json!({ "answers": { "Pergunta?": "Resposta" } }));
        ClarificationObserver.observe(&input, &ctx(root.to_str().unwrap()));
        assert!(material(root, "demo").is_none(), "no active unit, no record");
    }

    #[test]
    fn a_closed_unit_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_unit(root, "done", "Completed");
        bind_session(root, "s-1", "done");
        let input = ask_input("s-1", json!({ "answers": { "Pergunta?": "Resposta" } }));
        ClarificationObserver.observe(&input, &ctx(root.to_str().unwrap()));
        assert!(material(root, "done").is_none(), "a finished unit takes no new material");
    }

    #[test]
    fn no_project_is_failopen() {
        let dir = tempdir().unwrap();
        let input = ask_input("s-1", json!({ "answers": { "Pergunta?": "Resposta" } }));
        // Sem `.claude/` nenhum — sobreviver é o contrato.
        ClarificationObserver.observe(&input, &ctx(dir.path().to_str().unwrap()));
    }
}
