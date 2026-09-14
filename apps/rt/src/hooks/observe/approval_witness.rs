//! `approval_witness` — a testemunha da aprovação.
//!
//! No `PostToolUse` de uma pergunta com opções (`AskUserQuestion`), a resposta
//! do usuário chega pelo harness em `tool_response`, que o modelo não
//! escreve. Quando a spec atual está na fase de plano e o usuário escolheu
//! uma das opções oferecidas que aprova, a testemunha grava no `spec.ndjson`
//! um `state` com a fase `approved`, o autor `user` e a testemunha
//! `{question, answer}`: a pergunta e a opção escolhida. Não há comando nem
//! segundo passo. Em seguida, ela diz ao assistente para sugerir `/clear`: a
//! execução começa numa janela limpa, e a retomada lê o estado da spec.
//!
//! ## O que conta como aprovação
//!
//! Três fatos, todos juntos; na dúvida, nada é gravado.
//!
//! 1. **A spec espera aprovação.** A spec atual, pela escada única, está na
//!    fase `plan`, ou ainda não nasceu. É isso que diz qual spec é e que há
//!    uma aprovação pendente. O modelo não grava essa aprovação à mão: o
//!    `run write` recusa o tipo `state`.
//! 2. **Uma escolha de verdade.** A resposta é exatamente um dos rótulos que a
//!    própria pergunta de aprovação ofereceu, e nunca os de outra pergunta da
//!    mesma chamada. Texto livre, digitado na linha "Outro" ou nas notas,
//!    chega no mesmo lugar da resposta e nunca aprova, diga o que disser: uma
//!    mensagem que só falava da aprovação já forjou uma. Quando as opções
//!    oferecidas não se leem, nada foi oferecido e nada aprova.
//! 3. **A opção é a de aprovar.** O rótulo é, por inteiro, o do catálogo:
//!    "Aprovar" ou "Approve". "Não aprovar", "Don't approve" e "Aprovar
//!    depois" não aprovam.
//!
//! ## Só a pergunta de aprovação
//!
//! A testemunha age numa pergunta só: a de aprovação, feita com o texto do
//! catálogo, "Aprovar esta spec?" ou "Approve this spec?". Qualquer outra
//! pergunta passa calada, sem gravar e sem aviso, mesmo com a spec em plano:
//! uma opção como "Aprovação manual", numa pergunta sobre outra coisa, não é
//! a aprovação da spec.
//!
//! ## Os pontos abertos barram
//!
//! A testemunha não confere nada por conta própria: a aprovação passa pela
//! mesma gravação do `run write`, e a regra do núcleo recusa aprovar uma spec
//! com ponto do levantamento aberto. Na recusa, nada é gravado, e o motivo do
//! núcleo, com o código e a lacuna de cada ponto, vai ao assistente. A regra
//! lê a spec no checkout principal, também quando a pergunta é respondida num
//! worktree.
//!
//! Uma spec ainda sem nascimento — sem nenhum `state`, com ou sem arquivo de
//! eventos — recebe no "Aprovar" primeiro o nascimento, em plano, e depois a
//! aprovação.
//!
//! ## Nunca barra, nunca cala
//!
//! A testemunha é uma trava que nunca barra: devolve `Inject`, que chega ao
//! assistente, ou `Allow`. O texto de um gancho no stderr não chega ao
//! modelo, então tudo o que ela tem a dizer vai pelo `Inject`: a sugestão de
//! `/clear` depois de gravar; por que nada foi gravado quando a spec esperava
//! aprovação e a resposta não aprovou, ou quando a gravação foi recusada; e,
//! quando uma aprovação foi escolhida sem spec em plano, ou com a spec já
//! aprovada, que nada foi gravado. Uma pergunta cancelada não diz nada.

use std::path::Path;

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::spec_events::Refusal;
use mustard_core::domain::spec_state::{PhaseWriter, SpecState};
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use crate::hooks::write::write_gate::say;
use crate::shared::spec_state::DiskSpecState;

/// A testemunha da aprovação, no `PostToolUse` da pergunta com opções.
pub struct ApprovalWitness;

/// Onde a spec está diante da aprovação.
#[derive(Debug, PartialEq, Eq)]
enum Standing {
    /// Na fase de plano: a aprovação está pendente.
    Awaiting(String),
    /// Ainda sem nascimento: sem nenhum `state`, que a regra da trava lê em
    /// plano. Espera aprovação como uma em plano.
    Unborn(String),
    /// Já aprovada.
    Approved(String),
    /// Sem spec atual, ou numa fase em que nada espera aprovação.
    NoPlan,
}

/// A spec que a pergunta de aprovação decide, e onde ela está: a spec atual,
/// pela escada única. Outra spec ligada à sessão nunca é decidida por ali: a
/// sessão se liga a toda spec que um evento nomeia.
fn standing(root: &str, session: Option<&str>) -> Standing {
    let Some(spec) = DiskSpecState::new(Path::new(root)).active(session) else {
        return Standing::NoPlan;
    };
    // O estado que a trava lê, pela mesma função do portão: a spec sem nenhum
    // `state`, que a regra da trava lê em plano, espera aprovação como uma em
    // plano.
    let Some(state) = crate::shared::spec_state::lock_state(Path::new(root), &spec) else {
        return Standing::NoPlan;
    };
    if state.phase == Some("plan") && crate::shared::spec_state::unborn(Path::new(root), &spec) {
        Standing::Unborn(spec)
    } else if state.phase == Some("plan") {
        Standing::Awaiting(spec)
    } else if state.approved {
        Standing::Approved(spec)
    } else {
        Standing::NoPlan
    }
}

/// A pergunta é a de aprovação, com o texto do catálogo em um dos idiomas.
fn is_approval_question(question: &str) -> bool {
    [Locale::PtBr, Locale::EnUs]
        .into_iter()
        .any(|lang| translate("approval.question", lang).trim() == question.trim())
}

/// Os rótulos que a pergunta `question` ofereceu, lidos do `tool_input`, que
/// o harness devolve como o modelo escreveu: só as opções dessa pergunta,
/// nunca as de outra pergunta da mesma chamada. Cada opção é o `label` dela,
/// ou ela mesma quando é só texto. Vazio quando nada se lê: nada foi
/// oferecido, e nada aprova.
fn offered_for(input: &HookInput, question: &str) -> Vec<String> {
    input
        .tool_input
        .get("questions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|q| q.get("question").and_then(Value::as_str).is_some_and(|text| text.trim() == question.trim()))
        .flat_map(|q| q.get("options").and_then(Value::as_array).into_iter().flatten())
        .filter_map(|option| match option {
            Value::String(s) => Some(s.as_str()),
            other => other.get("label").and_then(Value::as_str),
        })
        .filter(|label| !label.trim().is_empty())
        .map(str::to_string)
        .collect()
}

/// A resposta é exatamente um dos rótulos oferecidos, sem os espaços das
/// pontas. Por inteiro: um pedaço deixaria passar o texto livre que cita a
/// opção dentro de uma frase.
fn is_offered(answer: &str, offered: &[String]) -> bool {
    offered.iter().any(|o| o.trim() == answer.trim())
}

/// A opção é a de aprovar: o rótulo do catálogo, "Aprovar" ou "Approve", por
/// inteiro.
fn is_approve_option(label: &str) -> bool {
    [Locale::PtBr, Locale::EnUs]
        .into_iter()
        .any(|lang| translate("approval.option", lang).trim() == label.trim())
}

/// Aprova a spec `spec`, que esperava aprovação: grava o nascimento quando a
/// spec ainda não tem fase (`unborn`) e depois a aprovação, pela gravação
/// única. Devolve o que dizer ao assistente: a sugestão de `/clear` depois de
/// gravar, ou, quando uma gravação foi recusada, o motivo do núcleo.
fn approve(root: &str, spec: &str, question: &str, answer: &str, unborn: bool, lang: Locale) -> String {
    // O nascimento, numa spec que ainda não nasceu; numa que já nasceu, a
    // branch e a base que faltam, quando a branch do checkout é a da spec.
    let born = crate::commands::spec_events::write::record_birth(Path::new(root), spec, None);
    let recorded = match born {
        Err(refusal) if unborn => Err(refusal),
        _ => record_approval(root, spec, question, answer),
    };
    match recorded {
        Ok(()) => say("approval.witness.clear", lang, &[("{spec}", spec)]),
        Err(refusal) => say("approval.witness.unmet", lang, &[("{spec}", spec), ("{unmet}", &refusal.message(lang))]),
    }
}

/// Grava a aprovação: um `state` com a fase `approved`, o autor `user` e a
/// testemunha, pela mesma gravação do `run write`. A recusa da gravação volta
/// com o motivo do núcleo.
fn record_approval(root: &str, spec: &str, question: &str, answer: &str) -> Result<(), Refusal> {
    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!("approved"));
    draft.insert("author".to_string(), json!("user"));
    draft.insert("witness".to_string(), json!({ "question": question, "answer": answer }));
    crate::commands::spec_events::write::record(Path::new(root), spec, "state", draft, PhaseWriter::Witness)
        .map(|_| ())
}

/// Por que nada foi gravado, quando a spec esperava aprovação e a resposta
/// não aprovou. `None` numa pergunta cancelada, que não respondeu nada.
///
/// Uma resposta que não é nenhuma das opções é texto livre, e o remédio é
/// escolher a opção; uma opção escolhida que não é a de aprovar é outra
/// coisa, e pode ser uma recusa de verdade.
fn decline_notice(spec: &str, labels: &[String], offered: &[String], lang: Locale) -> Option<String> {
    if labels.is_empty() {
        return None;
    }
    let selected = quote(labels);
    if !labels.iter().any(|l| is_offered(l, offered)) {
        let menu = if offered.is_empty() { "—".to_string() } else { quote(offered) };
        return Some(say(
            "approval.witness.free_text",
            lang,
            &[("{spec}", spec), ("{selected}", &selected), ("{offered}", &menu)],
        ));
    }
    Some(say("approval.witness.not_affirmative", lang, &[("{spec}", spec), ("{selected}", &selected)]))
}

/// Os rótulos entre aspas, separados por vírgula, cada um cortado: uma
/// resposta digitada pode ser uma mensagem inteira.
fn quote(values: &[String]) -> String {
    values
        .iter()
        .map(|v| format!("\"{}\"", truncate(v.trim())))
        .collect::<Vec<_>>()
        .join(", ")
}

fn truncate(s: &str) -> String {
    const MAX: usize = 80;
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let head: String = s.chars().take(MAX).collect();
    format!("{head}…")
}

impl Check for ApprovalWitness {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return Ok(Verdict::Allow);
        }
        // Só a pergunta de aprovação conta; qualquer outra passa calada.
        let Some(answer) =
            input.ask_answers().items.into_iter().find(|item| is_approval_question(&item.question))
        else {
            return Ok(Verdict::Allow);
        };
        let root = ctx.project_dir_or_cwd(input);
        let lang = ctx.config.language().text_or_default();
        let offered = offered_for(input, &answer.question);
        let chosen = answer.labels.iter().find(|l| is_offered(l, &offered) && is_approve_option(l));
        let context = match (standing(&root, input.session_id.as_deref()), chosen) {
            (Standing::Awaiting(spec), Some(label)) => {
                Some(approve(&root, &spec, &answer.question, label, false, lang))
            }
            (Standing::Unborn(spec), Some(label)) => Some(approve(&root, &spec, &answer.question, label, true, lang)),
            (Standing::Awaiting(spec) | Standing::Unborn(spec), None) => {
                decline_notice(&spec, &answer.labels, &offered, lang)
            }
            (Standing::Approved(spec), Some(_)) => {
                Some(say("approval.witness.already", lang, &[("{spec}", &spec)]))
            }
            (Standing::NoPlan, Some(_)) => Some(say("approval.witness.no_plan", lang, &[])),
            (Standing::Approved(_) | Standing::NoPlan, None) => None,
        };
        Ok(context.map_or(Verdict::Allow, |context| Verdict::Inject { context }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::context;
    use mustard_core::domain::spec_state::State;
    use mustard_core::io::spec_events as store;
    use mustard_core::ProjectConfig;
    use tempfile::{tempdir, TempDir};

    const SESSION: &str = "s-witness";
    const QUESTION: &str = "Aprovar esta spec?";

    /// Uma variável `MUSTARD_ACTIVE_SPEC` herdada responde antes da sessão.
    fn ambient_override() -> bool {
        std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some()
    }

    fn ctx(root: &Path) -> Ctx {
        let mut ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PostToolUse));
        ctx.config = ProjectConfig::load(root);
        ctx
    }

    fn lang(root: &Path) -> Locale {
        ProjectConfig::load(root).language().text_or_default()
    }

    /// A pergunta `question` com as opções `options` e a resposta `answer`,
    /// como o harness entrega: o menu no `tool_input` e a resposta à parte.
    fn ask_on(question: &str, options: &[&str], answer: Value) -> HookInput {
        let options: Vec<Value> = options.iter().map(|l| json!({ "label": l })).collect();
        HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("AskUserQuestion".to_string()),
            session_id: Some(SESSION.to_string()),
            tool_input: json!({ "questions": [{ "question": question, "header": "Spec", "options": options }] }),
            raw: json!({ "tool_response": { "questions": [], "answers": { question: answer } } }),
            ..HookInput::default()
        }
    }

    /// A pergunta de aprovação.
    fn ask(options: &[&str], answer: Value) -> HookInput {
        ask_on(QUESTION, options, answer)
    }

    fn approve_or_adjust(answer: &str) -> HookInput {
        ask(&["Aprovar", "Ajustar"], json!(answer))
    }

    fn record(root: &Path, fields: Value) {
        record_for(root, "epic", "state", fields);
    }

    fn record_for(root: &Path, spec: &str, event_type: &str, fields: Value) {
        let path = store::spec_file(root, spec).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        store::write(&path, event_type, fields.as_object().cloned().unwrap(), &[]).unwrap();
    }

    /// Uma spec `epic` ligada à sessão, com os estados `states`.
    fn spec_with(states: &[Value]) -> TempDir {
        let dir = tempdir().unwrap();
        for fields in states {
            record(dir.path(), fields.clone());
        }
        context::bind_session_spec(&dir.path().to_string_lossy(), SESSION, "epic");
        dir
    }

    fn in_plan() -> TempDir {
        spec_with(&[json!({ "phase": "plan", "branch": "feature/epic", "base": "dev" })])
    }

    fn state(root: &Path) -> State {
        DiskSpecState::new(root).state("epic").expect("the spec has its event file")
    }

    fn events(root: &Path) -> usize {
        std::fs::read_to_string(store::spec_file(root, "epic").unwrap()).unwrap().lines().count()
    }

    fn witness(root: &Path, input: &HookInput) -> Verdict {
        ApprovalWitness.evaluate(input, &ctx(root)).expect("never errors")
    }

    /// Só o rótulo do catálogo, por inteiro, é a opção de aprovar.
    #[test]
    fn only_the_catalog_label_is_the_approve_option() {
        for yes in ["Aprovar", "Approve", " Aprovar "] {
            assert!(is_approve_option(yes), "{yes}");
        }
        for no in ["Não aprovar", "Don't approve", "Aprovar depois", "APROVAR", "Desaprovar", "Ajustar"] {
            assert!(!is_approve_option(no), "{no}");
        }
    }

    /// "Não aprovar" não aprova, e numa chamada com duas perguntas o
    /// "Aprovar" de outra pergunta não conta para a de aprovação.
    #[test]
    fn a_negation_or_another_questions_option_never_approves() {
        if ambient_override() {
            return;
        }
        let dir = in_plan();
        let root = dir.path();
        witness(root, &ask(&["Aprovar", "Não aprovar"], json!("Não aprovar")));
        assert!(!state(root).approved, "\"Não aprovar\" is a refusal");

        let two = HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("AskUserQuestion".to_string()),
            session_id: Some(SESSION.to_string()),
            tool_input: json!({ "questions": [
                { "question": QUESTION, "options": [{ "label": "Ajustar" }, { "label": "Parar" }] },
                { "question": "Publicar a página?", "options": [{ "label": "Aprovar" }] }
            ] }),
            raw: json!({ "tool_response": { "answers": {
                QUESTION: "Aprovar",
                "Publicar a página?": "Aprovar"
            } } }),
            ..HookInput::default()
        };
        witness(root, &two);
        assert!(!state(root).approved, "the approval question never offered Aprovar");
    }

    /// "Aprovar" grava o estado aprovado pela própria resposta, sem comando
    /// nenhum: a fase, o autor e a testemunha com a pergunta e a opção.
    #[test]
    fn choosing_approve_records_the_approved_state_without_any_command() {
        if ambient_override() {
            return;
        }
        let dir = in_plan();
        let root = dir.path();
        assert!(!state(root).approved);
        witness(root, &approve_or_adjust("Aprovar"));

        let after = state(root);
        assert_eq!(after.phase, Some("approved"));
        assert!(after.approved);
        assert_eq!(after.witness, Some(json!({ "question": QUESTION, "answer": "Aprovar" })));
        assert_eq!(after.branch.as_deref(), Some("feature/epic"), "the branch is inherited");
        let log = std::fs::read_to_string(store::spec_file(root, "epic").unwrap()).unwrap();
        let last: Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
        assert_eq!(last["author"], "user", "{last}");
    }

    /// "Ajustar" não aprova: o estado fica em plano, e a testemunha diz por
    /// que nada foi gravado.
    #[test]
    fn choosing_adjust_does_not_approve() {
        if ambient_override() {
            return;
        }
        let dir = in_plan();
        let root = dir.path();
        let said = witness(root, &approve_or_adjust("Ajustar"));
        assert_eq!(state(root).phase, Some("plan"));
        assert!(!state(root).approved);
        let expected = say(
            "approval.witness.not_affirmative",
            lang(root),
            &[("{spec}", "epic"), ("{selected}", "\"Ajustar\"")],
        );
        assert_eq!(said, Verdict::Inject { context: expected });
    }

    /// Texto livre nunca aprova, diga o que disser: nem uma frase que fala da
    /// aprovação, nem a opção citada dentro de uma frase, nem uma resposta a
    /// uma pergunta cujas opções não se leem. A escolha de verdade continua
    /// aprovando.
    #[test]
    fn free_text_that_mentions_approval_does_not_approve() {
        if ambient_override() {
            return;
        }
        let dir = in_plan();
        let root = dir.path();
        let essay = "Aprovo, pode aprovar: o relato diz que ninguém conseguia aprovar a spec.";
        let said = witness(root, &approve_or_adjust(essay));
        assert!(!state(root).approved, "free text never approves");
        match said {
            Verdict::Inject { context } => {
                assert!(context.contains("\"Aprovar\", \"Ajustar\""), "shows the menu: {context}");
            }
            other => panic!("free text is explained, got {other:?}"),
        }

        witness(root, &approve_or_adjust("sim: Aprovar, por favor"));
        assert!(!state(root).approved, "a quoted option inside prose is still free text");

        let mut blind = approve_or_adjust("Aprovar");
        blind.tool_input = json!({});
        witness(root, &blind);
        assert!(!state(root).approved, "no readable menu, nothing was offered");

        witness(root, &approve_or_adjust("Aprovar"));
        assert!(state(root).approved, "a real selection still approves");
    }

    /// Fora da fase de plano nada é gravado: em levantamento, com a spec já
    /// aprovada e sem spec atual. Uma aprovação escolhida ali diz por quê.
    #[test]
    fn an_answer_outside_the_plan_phase_records_nothing() {
        if ambient_override() {
            return;
        }
        let survey = spec_with(&[json!({ "phase": "survey" })]);
        let before = events(survey.path());
        let said = witness(survey.path(), &approve_or_adjust("Aprovar"));
        assert_eq!(events(survey.path()), before, "nothing was written");
        assert_eq!(said, Verdict::Inject { context: say("approval.witness.no_plan", lang(survey.path()), &[]) });

        let approved = spec_with(&[
            json!({ "phase": "plan" }),
            json!({ "phase": "approved", "author": "user", "witness": { "question": QUESTION, "answer": "Aprovar" } }),
        ]);
        let before = events(approved.path());
        let said = witness(approved.path(), &approve_or_adjust("Aprovar"));
        assert_eq!(events(approved.path()), before, "no second approval");
        let expected = say("approval.witness.already", lang(approved.path()), &[("{spec}", "epic")]);
        assert_eq!(said, Verdict::Inject { context: expected });

        let none = tempdir().unwrap();
        let said = witness(none.path(), &approve_or_adjust("Aprovar"));
        assert!(!none.path().join(".claude").join("spec").exists(), "no spec, nothing written");
        assert!(matches!(said, Verdict::Inject { .. }));

        // Uma resposta qualquer, sem spec em plano, não diz nada.
        assert_eq!(witness(none.path(), &approve_or_adjust("Ajustar")), Verdict::Allow);
    }

    /// Depois de gravar a aprovação, a testemunha diz ao assistente para
    /// sugerir `/clear`. A pergunta feita em inglês conta do mesmo jeito.
    #[test]
    fn after_the_approval_the_witness_suggests_clear() {
        if ambient_override() {
            return;
        }
        let dir = in_plan();
        let root = dir.path();
        let said = witness(root, &ask_on("Approve this spec?", &["Approve", "Adjust"], json!(["Approve"])));
        let expected = say("approval.witness.clear", lang(root), &[("{spec}", "epic")]);
        assert_eq!(said, Verdict::Inject { context: expected.clone() });
        assert!(expected.contains("/clear"), "{expected}");
        assert!(state(root).approved);
    }

    /// Uma pergunta que não é a de aprovação nunca aprova e nunca fala,
    /// mesmo com a spec em plano e uma opção com a palavra da aprovação.
    #[test]
    fn another_question_never_approves_nor_speaks() {
        if ambient_override() {
            return;
        }
        let dir = in_plan();
        let root = dir.path();
        let before = events(root);
        for answer in ["Aprovação manual", "Automática"] {
            let input = ask_on("Como liberar o cadastro?", &["Aprovação manual", "Automática"], json!(answer));
            assert_eq!(witness(root, &input), Verdict::Allow, "{answer}");
        }
        assert_eq!(events(root), before, "nothing was written");
        assert!(!state(root).approved);

        let none = tempdir().unwrap();
        let input = ask_on("Como liberar o cadastro?", &["Aprovação manual"], json!("Aprovação manual"));
        assert_eq!(witness(none.path(), &input), Verdict::Allow, "no notice without a spec either");
    }

    /// A testemunha não confere mais as marcas do comando antigo de
    /// aprovação: uma spec com o `meta.json` de uma spec Full e sem o
    /// `.clarified` é aprovada pela resposta.
    #[test]
    fn a_spec_without_the_old_clarified_mark_is_approved_by_the_answer() {
        if ambient_override() {
            return;
        }
        let dir = in_plan();
        let root = dir.path();
        let spec_dir = root.join(".claude").join("spec").join("epic");
        std::fs::write(spec_dir.join("meta.json"), r#"{"scope":"full (wave plan)","stage":"Plan"}"#).unwrap();

        let said = witness(root, &approve_or_adjust("Aprovar"));
        let expected = say("approval.witness.clear", lang(root), &[("{spec}", "epic")]);
        assert_eq!(said, Verdict::Inject { context: expected });
        assert!(state(root).approved, "the answer approves");
    }

    /// Um ponto do levantamento aberto barra a aprovação: a gravação é
    /// recusada pela regra do núcleo, nada é gravado, e a testemunha diz ao
    /// assistente o motivo do núcleo, com o código e a lacuna do ponto.
    #[test]
    fn a_spec_with_an_open_point_is_not_approved_and_the_witness_says_which() {
        if ambient_override() {
            return;
        }
        let dir = in_plan();
        let root = dir.path();
        record_for(root, "epic", "message", json!({ "author": "user", "text": "Travar o merge." }));
        record_for(
            root,
            "epic",
            "point",
            json!({ "block": "limits", "gap": "Os limites, com os valores", "from": "gap", "status": "open",
                "origin": 2, "facts": [{ "text": "f", "source": "mensagem 2" }] }),
        );
        let before = events(root);
        let said = witness(root, &approve_or_adjust("Aprovar"));
        assert!(!state(root).approved, "nothing was recorded");
        let refusal =
            record_approval(&root.to_string_lossy(), "epic", QUESTION, "Aprovar").expect_err("the core rule refuses");
        let reason = refusal.message(lang(root));
        assert!(reason.contains("MSTD-POINT-0001") && reason.contains("Os limites, com os valores"), "{reason}");
        let expected = say("approval.witness.unmet", lang(root), &[("{spec}", "epic"), ("{unmet}", &reason)]);
        assert_eq!(said, Verdict::Inject { context: expected });
        assert_eq!(events(root), before);
    }

    /// Uma spec sem nascimento e com um ponto aberto não é aprovada: o motivo
    /// vai ao assistente, e a trava continua lendo a spec em plano.
    #[test]
    fn a_refused_approval_leaves_the_spec_locked_in_plan() {
        if ambient_override() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        record_for(root, "epic", "message", json!({ "author": "user", "text": "Travar o merge." }));
        record_for(
            root,
            "epic",
            "point",
            json!({ "block": "limits", "gap": "Os limites, com os valores", "from": "gap", "status": "open",
                "origin": 1, "facts": [{ "text": "f", "source": "mensagem 1" }] }),
        );
        context::bind_session_spec(&root.to_string_lossy(), SESSION, "epic");
        match witness(root, &approve_or_adjust("Aprovar")) {
            Verdict::Inject { context } => assert!(context.contains("MSTD-POINT-0001"), "names the point: {context}"),
            other => panic!("the refusal is explained, got {other:?}"),
        }
        let lock = crate::shared::spec_state::lock_state(root, "epic").expect("the spec has its event file");
        assert_eq!(lock.phase, Some("plan"), "the lock still reads plan");
        assert!(!lock.approved, "nothing approved");
    }

    /// Um arquivo de eventos sem nenhum `state` e sem `meta.json` é uma spec
    /// em plano, sem nascimento: o "Aprovar" grava o plano e depois a
    /// aprovação.
    #[test]
    fn a_spec_file_without_a_state_is_born_and_approved() {
        if ambient_override() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        record_for(root, "epic", "message", json!({ "author": "user", "text": "oi" }));
        context::bind_session_spec(&root.to_string_lossy(), SESSION, "epic");
        witness(root, &approve_or_adjust("Aprovar"));
        assert!(state(root).approved, "the spec with no phase is approved");
        let log = std::fs::read_to_string(store::spec_file(root, "epic").unwrap()).unwrap();
        let phases: Vec<String> =
            log.lines().filter_map(|l| serde_json::from_str::<Value>(l).unwrap()["phase"].as_str().map(str::to_string)).collect();
        assert_eq!(phases, ["plan", "approved"], "{log}");
    }

    /// Um ajuste ligado à sessão, com o `meta.json` que nomeia como mãe a
    /// spec da branch, não é aprovado por ali: a escada nomeia a spec da
    /// branch, já aprovada, e é ela que responde. O ajuste fica em plano, e a
    /// mãe fica como estava.
    #[test]
    fn a_fix_bound_to_the_session_is_not_approved_through_its_parents_branch() {
        if ambient_override() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        crate::shared::spec_state::stand_on_spec_branch(root, "epic-1");
        record_for(root, "epic-1", "state", json!({ "phase": "running", "branch": "feature/epic-1" }));
        record_for(root, "ajuste", "state", json!({ "phase": "plan", "branch": "feature/epic-1" }));
        std::fs::write(
            root.join(".claude").join("spec").join("ajuste").join("meta.json"),
            // A mãe gravada como veio, com espaço, barra e maiúscula.
            r#"{"scope":"light","stage":"Analyze","parent":" Epic-1/ "}"#,
        )
        .unwrap();
        context::bind_session_spec(&root.to_string_lossy(), SESSION, "ajuste");

        let said = witness(root, &approve_or_adjust("Aprovar"));
        let expected = say("approval.witness.already", lang(root), &[("{spec}", "epic-1")]);
        assert_eq!(said, Verdict::Inject { context: expected }, "the branch spec answers");
        let disk = DiskSpecState::new(root);
        assert!(!disk.state("ajuste").unwrap().approved, "the fix stays in plan");
        assert_eq!(disk.state("epic-1").unwrap().phase, Some("running"), "the parent is untouched");
    }

    /// Uma spec ligada à sessão que não é ajuste da spec da branch nunca é
    /// aprovada por ali: a sessão se liga a toda spec que um evento nomeia.
    #[test]
    fn a_bound_spec_that_is_not_a_fix_of_the_branch_spec_is_never_approved() {
        if ambient_override() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        crate::shared::spec_state::stand_on_spec_branch(root, "epic-1");
        record_for(root, "epic-1", "state", json!({ "phase": "running", "branch": "feature/epic-1" }));
        record_for(root, "outra", "state", json!({ "phase": "plan", "branch": "feature/outra" }));
        context::bind_session_spec(&root.to_string_lossy(), SESSION, "outra");

        let said = witness(root, &approve_or_adjust("Aprovar"));
        let expected = say("approval.witness.already", lang(root), &[("{spec}", "epic-1")]);
        assert_eq!(said, Verdict::Inject { context: expected }, "the branch spec answers");
        assert!(!DiskSpecState::new(root).state("outra").unwrap().approved, "the other spec stays in plan");
    }

    /// Respondida num worktree, a pergunta lê os pontos abertos da spec no
    /// checkout principal: com um ponto aberto lá, nada é aprovado, e a
    /// testemunha diz qual.
    #[test]
    fn in_a_worktree_the_witness_reads_the_open_points_of_the_main_checkout() {
        if ambient_override() {
            return;
        }
        let tmp = tempdir().unwrap();
        let main = tmp.path().join("repo");
        std::fs::create_dir_all(&main).unwrap();
        let git = |dir: &Path, args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            assert!(ok, "git {args:?} failed");
        };
        git(&main, &["init", "-q"]);
        git(&main, &["config", "user.email", "t@example.com"]);
        git(&main, &["config", "user.name", "t"]);
        git(&main, &["checkout", "-q", "-b", "dev"]);
        std::fs::write(main.join("README.md"), "oi\n").unwrap();
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-q", "-m", "init"]);
        std::fs::write(main.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#).unwrap();
        record(&main, json!({ "phase": "plan", "branch": "feature/epic" }));
        record_for(&main, "epic", "message", json!({ "author": "user", "text": "Travar o merge." }));
        record_for(
            &main,
            "epic",
            "point",
            json!({ "block": "limits", "gap": "Os limites, com os valores", "from": "gap", "status": "open",
                "origin": 2, "facts": [{ "text": "f", "source": "mensagem 2" }] }),
        );
        let wt = tmp.path().join("wt");
        git(&main, &["worktree", "add", "-q", &wt.to_string_lossy(), "-b", "feature/epic"]);

        match witness(&wt, &approve_or_adjust("Aprovar")) {
            Verdict::Inject { context } => assert!(context.contains("MSTD-POINT-0001"), "{context}"),
            other => panic!("the main checkout's spec has an open point, got {other:?}"),
        }
        assert!(!state(&main).approved, "nothing was recorded");
    }

    /// Uma pasta de spec antiga, só com o `meta.json` e o `spec.md`, fica
    /// livre, e nada espera aprovação nela: o "Aprovar" não grava nada, não
    /// cria o arquivo de eventos, e o `spec.md` fica como estava.
    #[test]
    fn an_old_folder_with_only_meta_json_is_not_approved_by_the_answer() {
        if ambient_override() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec_dir = root.join(".claude").join("spec").join("epic");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("meta.json"), r#"{"scope":"light","stage":"Plan","base":"dev"}"#).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# Epic\n").unwrap();
        context::bind_session_spec(&root.to_string_lossy(), SESSION, "epic");

        let said = witness(root, &approve_or_adjust("Aprovar"));
        assert_eq!(said, Verdict::Inject { context: say("approval.witness.no_plan", lang(root), &[]) });
        assert!(!spec_dir.join("spec.ndjson").exists(), "no event file is born");
        assert_eq!(std::fs::read_to_string(spec_dir.join("spec.md")).unwrap(), "# Epic\n", "the old document stays");
    }

    /// O nascimento pela testemunha nunca lê o `meta.json`: numa spec com o
    /// arquivo de eventos e sem `state`, um `meta.json` com base e mãe ao lado
    /// não entra no `state` gravado.
    #[test]
    fn a_birth_by_the_witness_never_reads_meta_json() {
        if ambient_override() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        record_for(root, "mae", "state", json!({ "phase": "running", "branch": "feature/mae" }));
        record_for(root, "epic", "message", json!({ "author": "user", "text": "oi" }));
        let meta = r#"{"scope":"light","stage":"Plan","base":"dev","parent":"mae"}"#;
        std::fs::write(root.join(".claude").join("spec").join("epic").join("meta.json"), meta).unwrap();
        context::bind_session_spec(&root.to_string_lossy(), SESSION, "epic");

        witness(root, &approve_or_adjust("Aprovar"));
        let after = state(root);
        assert!(after.approved, "the spec with no phase is approved");
        assert_eq!(after.base, None, "the base of the meta.json never enters");
        assert_eq!(after.branch, None, "the parent's branch never enters");
    }

    /// Uma pergunta cancelada não responde nada e não diz nada.
    #[test]
    fn a_dismissed_dialog_says_nothing() {
        if ambient_override() {
            return;
        }
        let dir = in_plan();
        let root = dir.path();
        assert_eq!(witness(root, &ask(&["Aprovar", "Ajustar"], json!({}))), Verdict::Allow);
        let mut cancelled = approve_or_adjust("Aprovar");
        cancelled.raw = json!({ "tool_response": { "answers": {} } });
        assert_eq!(witness(root, &cancelled), Verdict::Allow);
        assert!(!state(root).approved);
    }
}
