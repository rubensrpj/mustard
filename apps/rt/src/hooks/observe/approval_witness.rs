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
//!    fase `plan`. É isso que diz qual spec é e que há uma aprovação
//!    pendente; os arquivos da spec só o binário grava, então o modelo não
//!    muda esse estado à mão.
//! 2. **Uma escolha de verdade.** A resposta é exatamente um dos rótulos que
//!    a pergunta ofereceu. Texto livre, digitado na linha "Outro" ou nas
//!    notas, chega no mesmo lugar da resposta e nunca aprova, diga o que
//!    disser: uma mensagem que só falava da aprovação já forjou uma. Quando
//!    as opções oferecidas não se leem, nada foi oferecido e nada aprova.
//! 3. **A opção aprova.** Alguma palavra do rótulo começa por `approv` ou
//!    `aprov`. Por palavra, e não por pedaço: "Desaprovar", "Reprovar",
//!    "Ajustar" e "Reject" não aprovam; "Aprovar" e "Approve" aprovam. A
//!    raiz só separa aprovar de recusar dentro de uma escolha de verdade; o
//!    peso está nos fatos 1 e 2.
//!
//! ## Nunca barra, nunca cala
//!
//! A testemunha é uma trava que nunca barra: devolve `Inject`, que chega ao
//! assistente, ou `Allow`. O texto de um gancho no stderr não chega ao
//! modelo, então tudo o que ela tem a dizer vai pelo `Inject`: a sugestão de
//! `/clear` depois de gravar; por que nada foi gravado quando a spec esperava
//! aprovação e a resposta não aprovou; e, quando uma aprovação foi escolhida
//! sem spec em plano, ou com a spec já aprovada, que nada foi gravado. Uma
//! pergunta cancelada, ou uma resposta qualquer sem spec em plano, não diz
//! nada: a testemunha vê todas as perguntas da sessão.

use std::path::Path;

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::spec_state::SpecState;
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::Locale;
use serde_json::{json, Value};

use crate::hooks::write::write_gate::say;
use crate::shared::spec_state::DiskSpecState;

/// A testemunha da aprovação, no `PostToolUse` da pergunta com opções.
pub struct ApprovalWitness;

/// As raízes da aprovação nos idiomas do projeto. Uma opção com uma palavra
/// que começa por uma delas aprova.
const APPROVAL_STEMS: &[&str] = &["approv", "aprov"];

/// Onde a spec atual está diante da aprovação.
#[derive(Debug, PartialEq, Eq)]
enum Standing {
    /// Na fase de plano: a aprovação está pendente.
    Awaiting(String),
    /// Já aprovada.
    Approved(String),
    /// Sem spec atual, sem estado, ou numa fase em que nada espera
    /// aprovação.
    NoPlan,
}

/// A spec atual da sessão, pela escada única, e onde ela está diante da
/// aprovação.
fn standing(root: &str, session: Option<&str>) -> Standing {
    let disk = DiskSpecState::new(Path::new(root));
    let Some(spec) = disk.active(session) else {
        return Standing::NoPlan;
    };
    match disk.state(&spec) {
        Some(state) if state.phase == Some("plan") => Standing::Awaiting(spec),
        Some(state) if state.approved => Standing::Approved(spec),
        _ => Standing::NoPlan,
    }
}

/// Todos os rótulos que o usuário escolheu; vazio numa pergunta cancelada.
fn selected_labels(input: &HookInput) -> Vec<String> {
    input.ask_answers().items.into_iter().flat_map(|item| item.labels).collect()
}

/// Todos os rótulos que a pergunta ofereceu, lidos do `tool_input`, que o
/// harness devolve como o modelo escreveu.
///
/// Anda pelo documento atrás de toda lista `options` e pega o `label` de
/// cada opção, ou a própria opção quando ela é só texto; assim não importa
/// onde a lista mora. Vazio quando nada se lê: nada foi oferecido, e nada
/// aprova.
fn offered_labels(input: &HookInput) -> Vec<String> {
    fn walk(node: &Value, out: &mut Vec<String>) {
        match node {
            Value::Object(map) => {
                for (key, value) in map {
                    if key == "options"
                        && let Some(items) = value.as_array()
                    {
                        for item in items {
                            let label = match item {
                                Value::String(s) => Some(s.as_str()),
                                other => other.get("label").and_then(Value::as_str),
                            };
                            if let Some(l) = label.filter(|l| !l.trim().is_empty()) {
                                out.push(l.to_string());
                            }
                        }
                    }
                    walk(value, out);
                }
            }
            Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(&input.tool_input, &mut out);
    out
}

/// A resposta é exatamente um dos rótulos oferecidos, sem os espaços das
/// pontas. Por inteiro: um pedaço deixaria passar o texto livre que cita a
/// opção dentro de uma frase.
fn is_offered(answer: &str, offered: &[String]) -> bool {
    offered.iter().any(|o| o.trim() == answer.trim())
}

/// Alguma palavra do rótulo, em minúsculas, começa por uma raiz da
/// aprovação. Só é perguntado de uma resposta que já passou por
/// [`is_offered`].
fn is_affirmative(label: &str) -> bool {
    label
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .any(|w| APPROVAL_STEMS.iter().any(|&stem| w.starts_with(stem)))
}

/// A pergunta e a opção que aprova, quando o usuário escolheu uma.
fn chosen_approval(input: &HookInput, offered: &[String]) -> Option<(String, String)> {
    input.ask_answers().items.into_iter().find_map(|item| {
        let label = item
            .labels
            .into_iter()
            .find(|l| is_offered(l, offered) && is_affirmative(l))?;
        Some((item.question, label))
    })
}

/// Grava a aprovação: um `state` com a fase `approved`, o autor `user` e a
/// testemunha, pela mesma gravação do `run write`. `false` quando a gravação
/// foi recusada.
fn record_approval(root: &str, spec: &str, question: &str, answer: &str) -> bool {
    let Value::Object(draft) = json!({
        "phase": "approved",
        "author": "user",
        "witness": { "question": question, "answer": answer },
    }) else {
        return false;
    };
    crate::commands::spec_events::write::record(Path::new(root), spec, "state", draft).is_ok()
}

/// Até os leitores passarem ao estado, o mesmo gesto grava também a marca de
/// aprovação que eles leem.
fn mint_marker(root: &str, spec: &str, session: Option<&str>) {
    if let Some(marker) = crate::shared::context::approval_marker_path(root, spec) {
        let body = crate::shared::context::marker_body(
            spec,
            "AskUserQuestion",
            session.unwrap_or("unknown"),
            &mustard_core::time::now_iso8601(),
        );
        let _ = mustard_core::io::fs::write_atomic(&marker, body.as_bytes());
    }
}

/// Por que nada foi gravado, quando a spec esperava aprovação e a resposta
/// não aprovou. `None` numa pergunta cancelada, que não respondeu nada.
///
/// Uma resposta que não é nenhuma das opções é texto livre, e o remédio é
/// escolher a opção; uma opção escolhida sem a raiz da aprovação é outra
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
        let root = ctx.project_dir_or_cwd(input);
        let lang = ctx.config.language().text_or_default();
        let session = input.session_id.as_deref();
        let offered = offered_labels(input);
        let chosen = chosen_approval(input, &offered);
        let context = match (standing(&root, session), chosen) {
            (Standing::Awaiting(spec), Some((question, answer))) => {
                let recorded = record_approval(&root, &spec, &question, &answer);
                mint_marker(&root, &spec, session);
                recorded.then(|| say("approval.witness.clear", lang, &[("{spec}", &spec)]))
            }
            (Standing::Awaiting(spec), None) => {
                decline_notice(&spec, &selected_labels(input), &offered, lang)
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

    /// A pergunta com as opções `options` e a resposta `answer`, como o
    /// harness entrega: o menu no `tool_input` e a resposta à parte.
    fn ask(options: &[&str], answer: Value) -> HookInput {
        let options: Vec<Value> = options.iter().map(|l| json!({ "label": l })).collect();
        HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("AskUserQuestion".to_string()),
            session_id: Some(SESSION.to_string()),
            tool_input: json!({ "questions": [{ "question": QUESTION, "header": "Spec", "options": options }] }),
            raw: json!({ "tool_response": { "questions": [], "answers": { QUESTION: answer } } }),
            ..HookInput::default()
        }
    }

    fn approve_or_adjust(answer: &str) -> HookInput {
        ask(&["Aprovar", "Ajustar"], json!(answer))
    }

    fn record(root: &Path, fields: Value) {
        let path = store::spec_file(root, "epic").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        store::write(&path, "state", fields.as_object().cloned().unwrap(), &[]).unwrap();
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

    #[test]
    fn affirmative_matches_approve_words_across_languages() {
        for yes in ["Aprovar", "Aprovar e implementar agora", "Approve", "Approve only", "APROVAR"] {
            assert!(is_affirmative(yes), "should be affirmative: {yes}");
        }
    }

    #[test]
    fn affirmative_rejects_negations_and_stops() {
        for no in ["Ajustar", "Rejeitar", "Reject", "Stop", "Desaprovar", "Reprovar", "Disapprove"] {
            assert!(!is_affirmative(no), "should NOT be affirmative: {no}");
        }
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
        assert!(is_affirmative(essay), "the stem fires on the text; the shape check must stop it");
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
    /// sugerir `/clear`.
    #[test]
    fn after_the_approval_the_witness_suggests_clear() {
        if ambient_override() {
            return;
        }
        let dir = in_plan();
        let root = dir.path();
        let said = witness(root, &ask(&["Approve", "Adjust"], json!(["Approve"])));
        let expected = say("approval.witness.clear", lang(root), &[("{spec}", "epic")]);
        assert_eq!(said, Verdict::Inject { context: expected.clone() });
        assert!(expected.contains("/clear"), "{expected}");
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

    /// O mesmo gesto grava as duas coisas, enquanto os leitores não passaram
    /// ao estado: o `state` aprovado e a marca que eles leem.
    #[test]
    fn the_same_gesture_records_the_state_and_the_marker() {
        if ambient_override() {
            return;
        }
        let dir = in_plan();
        let root = dir.path();
        witness(root, &approve_or_adjust("Aprovar"));
        assert!(state(root).approved);
        let marker = context::approval_marker_path(&root.to_string_lossy(), "epic").unwrap();
        assert!(marker.is_file(), "the marker the readers still read is minted too");
    }
}
