//! `approval_witness` — a testemunha dos gestos de aprovação.
//!
//! No `PostToolUse` de uma pergunta com opções (`AskUserQuestion`), a resposta
//! do usuário chega pelo harness em `tool_response`, que o modelo não
//! escreve. É por aqui que o Mustard sabe que o usuário disse "sim": pelo
//! clique numa das opções que a pergunta ofereceu, e nunca pela leitura que o
//! modelo faz de uma frase.
//!
//! ## Os gestos são uma lista
//!
//! Cada gesto é um item de [`GESTURES`]: como a pergunta é reconhecida, a
//! opção que dá o "sim" e o que fazer com a resposta. Acrescentar um gesto é
//! somar um item. Hoje são dois:
//!
//! - **"Aprovar esta spec?"**, com "Aprovar" e "Ajustar". Com a spec em plano,
//!   o "Aprovar" grava no `spec.ndjson` um `state` com a fase `approved`, o
//!   autor `user` e a testemunha `{question, answer}`, e a testemunha diz ao
//!   assistente para sugerir `/clear`.
//! - **a mudança que uma onda propõe**, com "Aceitar" e "Recusar". A rodada
//!   para quando uma onda diz que o plano dela não funciona, e só segue
//!   depois do clique em "Aceitar" gravado aqui. O enunciado é escrito com as
//!   palavras que o usuário entender e nunca é comparado: quem diz qual
//!   mudança o clique decide é o código no cabeçalho da pergunta, que a
//!   testemunha guarda ao lado da resposta.
//!
//! ## A resposta de cada pergunta
//!
//! Toda pergunta com opções respondida, de gesto ou qualquer outra, vai para
//! o bloco da conversa da spec atual como mensagem do usuário: a pergunta, a
//! resposta e a nota, quando ele escreveu uma. Numa pergunta de gesto, o
//! clique numa das opções oferecidas grava a mensagem com a testemunha — a
//! pergunta e a opção —, e é essa mensagem que a rodada lê como o "sim" da
//! mudança. Texto livre também é gravado, sem testemunha: gravar o que o
//! usuário disse não destrava nada. Uma pergunta cancelada não grava nada.
//!
//! ## O que conta como "sim"
//!
//! Dois fatos, os dois juntos; na dúvida, nada é aceito.
//!
//! 1. **Uma escolha de verdade.** A resposta é exatamente um dos rótulos que a
//!    própria pergunta ofereceu, e nunca os de outra pergunta da mesma
//!    chamada. Texto livre, digitado na linha "Outro" ou nas notas, chega no
//!    mesmo lugar da resposta e nunca conta, diga o que disser: uma mensagem
//!    que só falava da aprovação já forjou uma. Quando as opções oferecidas
//!    não se leem, nada foi oferecido e nada conta.
//! 2. **A opção é a do "sim".** O rótulo é, por inteiro, o do catálogo:
//!    "Aprovar" ou "Approve", "Aceitar" ou "Accept". "Não aprovar", "Don't
//!    approve" e "Aprovar depois" não contam.
//!
//! A aprovação da spec pede ainda um terceiro: a spec atual, pela escada
//! única, está na fase `plan`, ou ainda não nasceu. O modelo não grava essa
//! aprovação à mão: o `run write` recusa o tipo `state`, e também toda
//! mensagem com a testemunha, de qualquer autor, para gravar, rever ou tirar.
//!
//! ## Só as perguntas dos gestos
//!
//! A testemunha decide só nas perguntas do catálogo. Qualquer outra pergunta
//! passa calada, sem aviso, mesmo com a spec em plano: uma opção como
//! "Aprovação manual", numa pergunta sobre outra coisa, não é gesto nenhum.
//!
//! ## Os pontos abertos barram
//!
//! A aprovação passa pela mesma gravação do `run write`, e a regra do núcleo
//! recusa aprovar uma spec com ponto do levantamento aberto. Na recusa, nada é
//! gravado, e o motivo do núcleo, com o código e a lacuna de cada ponto, vai
//! ao assistente. A regra lê a spec no checkout principal, também quando a
//! pergunta é respondida num worktree.
//!
//! Uma spec ainda sem nascimento — sem nenhum `state`, com ou sem arquivo de
//! eventos — recebe no "Aprovar" a aprovação direto, na mesma gravação única.
//! Gravada a aprovação, a branch que falta entra no estado, quando a branch do
//! checkout é a desta spec.
//!
//! ## Nunca barra, nunca cala
//!
//! A testemunha é uma trava que nunca barra: devolve `Inject`, que chega ao
//! assistente, ou `Allow`. O texto de um gancho no stderr não chega ao
//! modelo, então tudo o que ela tem a dizer vai pelo `Inject`: o que foi
//! gravado, e por que nada foi quando a resposta não contou. Uma pergunta
//! cancelada não diz nada.

use std::path::Path;

use mustard_core::domain::model::contract::{AskAnswer, Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::spec_events::Refusal;
use mustard_core::domain::spec_state::{PhaseWriter, SpecState};
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use crate::commands::flow::round::change_code_of;
use crate::commands::spec_events::conversation::{record_message, record_witnessed_message};
use crate::hooks::write::write_gate::say;
use crate::shared::spec_state::DiskSpecState;

/// A testemunha dos gestos de aprovação, no `PostToolUse` da pergunta com
/// opções.
pub struct ApprovalWitness;

/// A vaga da pergunta de um gesto, que diz sobre o que o clique decide.
const SLOT: &str = "{code}";

/// Como a pergunta de um gesto é reconhecida.
enum Keyed {
    /// Pela frase do catálogo, que pode ter a vaga [`SLOT`]: o que a vaga
    /// traz é o que o gesto decide.
    Phrase(&'static str),
    /// Pelas duas opções do catálogo — a do "sim" e esta, a do "não" — e pelo
    /// código no cabeçalho da pergunta. O enunciado nunca é comparado: quem
    /// pergunta o escreve com as palavras que o usuário entender.
    Options(&'static str),
}

/// Um gesto de aprovação: como a pergunta é reconhecida, a opção que dá o
/// "sim" e o que fazer com a resposta.
struct Gesture {
    keyed: Keyed,
    /// A chave da opção que dá o "sim".
    yes: &'static str,
    /// O que o gesto faz com a resposta; devolve o que dizer ao assistente.
    decide: fn(&Answered<'_>) -> Option<String>,
}

/// Os gestos de aprovação. Acrescentar um gesto é somar um item.
const GESTURES: &[Gesture] = &[
    // "Aprovar/Ajustar": a aprovação da spec.
    Gesture { keyed: Keyed::Phrase("approval.question"), yes: "approval.option", decide: decide_approval },
    // "Aceitar/Recusar": a mudança que parte de um agente.
    Gesture { keyed: Keyed::Options("change.decline"), yes: "change.accept", decide: decide_change },
];

/// O que o usuário fez na pergunta de um gesto.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Choice {
    /// Clicou na opção do "sim"; guarda o rótulo.
    Yes(String),
    /// Clicou noutra das opções oferecidas.
    Other,
    /// Respondeu com texto livre: nada do que veio é uma opção oferecida.
    Free,
    /// Cancelou a pergunta.
    Nothing,
}

/// Uma pergunta de gesto respondida, com tudo o que o gesto precisa para
/// decidir.
struct Answered<'a> {
    root: &'a str,
    session: Option<&'a str>,
    lang: Locale,
    /// A pergunta, como o harness a devolveu.
    question: &'a str,
    /// O que a vaga da pergunta trouxe; vazio na pergunta sem vaga.
    slot: String,
    /// Os rótulos escolhidos, como vieram.
    labels: &'a [String],
    /// Os rótulos que a pergunta ofereceu.
    offered: Vec<String>,
    choice: Choice,
    /// A resposta foi gravada na conversa da spec atual.
    recorded: bool,
}

/// O que a vaga de `template` traz em `question`, quando o texto do catálogo
/// está em `question`, com ou sem explicação antes, depois ou em volta dele:
/// vazio num modelo sem vaga. A vaga é uma palavra só, sem espaço, colada ao
/// resto do texto do catálogo — explicação não entra no meio dele.
fn slot_of(template: &str, question: &str) -> Option<String> {
    let question = question.trim();
    let Some((head, tail)) = template.split_once(SLOT) else {
        return question.contains(template.trim()).then(String::new);
    };
    let head = head.trim_start();
    let tail = tail.trim_end();
    let after_head = &question[question.find(head)?..][head.len()..];
    let middle = after_head[..after_head.find(tail)?].trim();
    (!middle.is_empty() && !middle.chars().any(char::is_whitespace)).then(|| middle.to_string())
}

/// O gesto da pergunta respondida e o que ele decide: a spec, na pergunta de
/// aprovação, que se reconhece pela frase do catálogo; o código da mudança,
/// na pergunta da mudança, que se reconhece pelas duas opções do catálogo e
/// traz o código no cabeçalho. Vazio no lugar do código quando o cabeçalho
/// não traz nenhum: o gesto é da mudança do mesmo jeito, e a testemunha diz
/// ao assistente o que fazer.
fn gesture_of(question: &str, offered: &[String], header: &str) -> Option<(&'static Gesture, String)> {
    GESTURES.iter().find_map(|gesture| match gesture.keyed {
        Keyed::Phrase(key) => [Locale::PtBr, Locale::EnUs]
            .into_iter()
            .find_map(|lang| slot_of(translate(key, lang), question))
            .map(|slot| (gesture, slot)),
        Keyed::Options(no) => {
            offers_both(offered, gesture.yes, no).then(|| (gesture, change_code_of(header).unwrap_or_default()))
        }
    })
}

/// A pergunta ofereceu as duas opções do gesto, as do catálogo e no mesmo
/// idioma.
fn offers_both(offered: &[String], yes: &'static str, no: &'static str) -> bool {
    [Locale::PtBr, Locale::EnUs].into_iter().any(|lang| {
        let (yes, no) = (translate(yes, lang), translate(no, lang));
        offered.iter().any(|label| label.trim() == yes) && offered.iter().any(|label| label.trim() == no)
    })
}

/// A opção é a do "sim" do gesto: o rótulo do catálogo, por inteiro, num dos
/// idiomas.
fn is_yes(gesture: &Gesture, label: &str) -> bool {
    [Locale::PtBr, Locale::EnUs].into_iter().any(|lang| translate(gesture.yes, lang).trim() == label.trim())
}

/// Os rótulos que a pergunta `question` ofereceu, lidos do `tool_input`, que
/// o harness devolve como o modelo escreveu: só as opções dessa pergunta,
/// nunca as de outra pergunta da mesma chamada. Cada opção é o `label` dela,
/// ou ela mesma quando é só texto. Vazio quando nada se lê: nada foi
/// oferecido, e nada conta.
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

/// O cabeçalho da pergunta `question`, lido do `tool_input`: é ele que leva o
/// código da mudança, fora do enunciado. Vazio quando a pergunta não o traz.
fn header_for(input: &HookInput, question: &str) -> String {
    input
        .tool_input
        .get("questions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|q| q.get("question").and_then(Value::as_str).is_some_and(|text| text.trim() == question.trim()))
        .find_map(|q| q.get("header").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

/// A resposta é exatamente um dos rótulos oferecidos, sem os espaços das
/// pontas. Por inteiro: um pedaço deixaria passar o texto livre que cita a
/// opção dentro de uma frase.
fn is_offered(answer: &str, offered: &[String]) -> bool {
    offered.iter().any(|o| o.trim() == answer.trim())
}

/// O que o usuário fez na pergunta de `gesture`.
fn choice_of(gesture: &Gesture, labels: &[String], offered: &[String]) -> Choice {
    if labels.is_empty() {
        return Choice::Nothing;
    }
    if let Some(yes) = labels.iter().find(|l| is_offered(l, offered) && is_yes(gesture, l)) {
        return Choice::Yes(yes.trim().to_string());
    }
    if labels.iter().any(|l| is_offered(l, offered)) {
        return Choice::Other;
    }
    Choice::Free
}

// ---------------------------------------------------------------------------
// O gesto da aprovação da spec
// ---------------------------------------------------------------------------

/// Onde a spec está diante da aprovação.
#[derive(Debug, PartialEq, Eq)]
enum Standing {
    /// Na fase de plano: a aprovação está pendente. A spec sem nenhum `state`
    /// entra aqui, porque a regra da trava a lê em plano.
    Awaiting(String),
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
    if state.phase == Some("plan") {
        Standing::Awaiting(spec)
    } else if state.approved {
        Standing::Approved(spec)
    } else {
        Standing::NoPlan
    }
}

/// A aprovação da spec: com a spec em plano, o "Aprovar" grava a aprovação;
/// fora do plano, a testemunha diz por que nada foi gravado.
fn decide_approval(answer: &Answered<'_>) -> Option<String> {
    let lang = answer.lang;
    match (standing(answer.root, answer.session), &answer.choice) {
        (Standing::Awaiting(spec), Choice::Yes(label)) => {
            Some(approve(answer.root, &spec, answer.question, label, lang))
        }
        (Standing::Awaiting(spec), _) => decline_notice(&spec, answer, lang),
        (Standing::Approved(spec), Choice::Yes(_)) => {
            Some(say("approval.witness.already", lang, &[("{spec}", &spec)]))
        }
        (Standing::NoPlan, Choice::Yes(_)) => Some(say("approval.witness.no_plan", lang, &[])),
        (Standing::Approved(_) | Standing::NoPlan, _) => None,
    }
}

/// Aprova a spec `spec`, que esperava aprovação: uma gravação só, a da
/// aprovação, tanto na spec que já nasceu quanto na que ainda não tem fase.
/// Recusada, ela não deixa nada no arquivo, e a recusa diz a verdade sobre o
/// próprio efeito. Devolve o que dizer ao assistente: a sugestão de `/clear`
/// depois de gravar, ou, na recusa, o motivo do núcleo.
fn approve(root: &str, spec: &str, question: &str, answer: &str, lang: Locale) -> String {
    match record_approval(root, spec, question, answer) {
        Ok(()) => {
            // Gravada a aprovação, a branch que falta entra no estado, quando
            // a branch do checkout é a desta spec.
            let _ = crate::commands::spec_events::write::record_birth(Path::new(root), spec, None);
            say("approval.witness.clear", lang, &[("{spec}", spec)])
        }
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
fn decline_notice(spec: &str, answer: &Answered<'_>, lang: Locale) -> Option<String> {
    let selected = quote(answer.labels);
    match answer.choice {
        Choice::Nothing | Choice::Yes(_) => None,
        Choice::Free => Some(say(
            "approval.witness.free_text",
            lang,
            &[("{spec}", spec), ("{selected}", &selected), ("{offered}", &menu(&answer.offered))],
        )),
        Choice::Other => {
            Some(say("approval.witness.not_affirmative", lang, &[("{spec}", spec), ("{selected}", &selected)]))
        }
    }
}

// ---------------------------------------------------------------------------
// O gesto da mudança que parte de um agente
// ---------------------------------------------------------------------------

/// A mudança que parte de um agente: o clique já foi gravado com a resposta,
/// e a rodada o lê. Aqui a testemunha só diz ao assistente o que aconteceu.
fn decide_change(answer: &Answered<'_>) -> Option<String> {
    let code = answer.slot.as_str();
    let lang = answer.lang;
    match answer.choice {
        Choice::Nothing => None,
        // Sem o código no cabeçalho, nada diz qual mudança o clique decide:
        // a resposta vai para a conversa, e nenhuma rodada a lê como o "sim".
        _ if code.is_empty() => Some(say("change.witness.no_code", lang, &[])),
        Choice::Free => Some(say(
            "change.witness.free_text",
            lang,
            &[("{code}", code), ("{selected}", &quote(answer.labels)), ("{offered}", &menu(&answer.offered))],
        )),
        _ if !answer.recorded => Some(say("change.witness.no_spec", lang, &[("{code}", code)])),
        Choice::Yes(_) => Some(say("change.witness.accepted", lang, &[("{code}", code)])),
        Choice::Other => Some(say("change.witness.declined", lang, &[("{code}", code)])),
    }
}

// ---------------------------------------------------------------------------
// A resposta gravada
// ---------------------------------------------------------------------------

/// Os rótulos entre aspas, separados por vírgula, cada um cortado: uma
/// resposta digitada pode ser uma mensagem inteira.
fn quote(values: &[String]) -> String {
    values
        .iter()
        .map(|v| format!("\"{}\"", truncate(v.trim())))
        .collect::<Vec<_>>()
        .join(", ")
}

/// As opções oferecidas entre aspas, ou um traço quando nada se leu.
fn menu(offered: &[String]) -> String {
    if offered.is_empty() { "—".to_string() } else { quote(offered) }
}

fn truncate(s: &str) -> String {
    const MAX: usize = 80;
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let head: String = s.chars().take(MAX).collect();
    format!("{head}…")
}

/// Grava a pergunta respondida como mensagem do usuário, na spec atual: a
/// pergunta, a resposta (várias escolhas separadas por vírgula) e a nota,
/// cada uma numa linha. Com `witness`, a mensagem leva a testemunha: a
/// pergunta e a opção clicada. Pergunta sem resposta não grava nada; `true`
/// quando gravou.
fn record_answer(
    root: &Path,
    session: Option<&str>,
    item: &AskAnswer,
    witness: Option<&str>,
    change: Option<&str>,
) -> bool {
    let question = item.question.trim();
    let answer = item.labels.iter().map(|label| label.trim()).collect::<Vec<_>>().join(", ");
    if question.is_empty() || answer.is_empty() {
        return false;
    }
    let mut text = format!("{question}\n{answer}");
    if let Some(notes) = item.notes.as_deref().map(str::trim).filter(|notes| !notes.is_empty()) {
        text.push('\n');
        text.push_str(notes);
    }
    match witness {
        Some(clicked) => record_witnessed_message(root, session, &text, question, clicked, change).is_some(),
        None => record_message(root, session, &text).is_some(),
    }
}

/// A opção clicada que vai na testemunha: uma escolha só, e ela é uma das
/// oferecidas. Texto livre e escolha múltipla não levam testemunha.
fn clicked(labels: &[String], offered: &[String]) -> Option<String> {
    match labels {
        [one] if is_offered(one, offered) => Some(one.trim().to_string()),
        _ => None,
    }
}

impl Check for ApprovalWitness {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return Ok(Verdict::Allow);
        }
        let root = ctx.project_dir_or_cwd(input);
        let session = input.session_id.as_deref();
        let lang = ctx.config.language().text_or_default();
        let mut said: Vec<String> = Vec::new();
        for item in input.ask_answers().items {
            let offered = offered_for(input, &item.question);
            let header = header_for(input, &item.question);
            let Some((gesture, slot)) = gesture_of(&item.question, &offered, &header) else {
                // Uma pergunta que não é de gesto só vai para a conversa.
                record_answer(Path::new(&root), session, &item, None, None);
                continue;
            };
            let witness = clicked(&item.labels, &offered);
            // O código da mudança fica ao lado da resposta: é por ele, e não
            // pela frase mostrada, que a rodada reconhece o "sim".
            let change = Some(slot.as_str()).filter(|code| !code.is_empty() && matches!(gesture.keyed, Keyed::Options(_)));
            let recorded = record_answer(Path::new(&root), session, &item, witness.as_deref(), change);
            let answered = Answered {
                root: &root,
                session,
                lang,
                question: &item.question,
                slot,
                labels: &item.labels,
                choice: choice_of(gesture, &item.labels, &offered),
                offered,
                recorded,
            };
            said.extend((gesture.decide)(&answered));
        }
        Ok(if said.is_empty() { Verdict::Allow } else { Verdict::Inject { context: said.join("\n\n") } })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::context::session::bind_session_spec;
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
        ask_with(question, "Spec", options, answer)
    }

    /// [`ask_on`] com o cabeçalho `header`, que é onde vai o código da
    /// mudança.
    fn ask_with(question: &str, header: &str, options: &[&str], answer: Value) -> HookInput {
        let options: Vec<Value> = options.iter().map(|l| json!({ "label": l })).collect();
        HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("AskUserQuestion".to_string()),
            session_id: Some(SESSION.to_string()),
            tool_input: json!({ "questions": [{ "question": question, "header": header, "options": options }] }),
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
        bind_session_spec(&dir.path().to_string_lossy(), SESSION, "epic");
        dir
    }

    fn in_plan() -> TempDir {
        spec_with(&[json!({ "phase": "plan", "branch": "feature/epic", "base": "dev" })])
    }

    fn state(root: &Path) -> State {
        DiskSpecState::new(root).state("epic").expect("the spec has its event file")
    }

    /// As linhas do arquivo da spec fora das mensagens: a resposta de cada
    /// pergunta sempre vai para a conversa, e o que estes testes olham é o
    /// que a aprovação grava.
    fn outside_messages(file: &Path) -> Vec<String> {
        std::fs::read_to_string(file)
            .unwrap()
            .lines()
            .filter(|line| serde_json::from_str::<Value>(line).is_ok_and(|v| v["type"] != "message"))
            .map(str::to_string)
            .collect()
    }

    fn events(root: &Path) -> usize {
        outside_messages(&store::spec_file(root, "epic").unwrap()).len()
    }

    fn witness(root: &Path, input: &HookInput) -> Verdict {
        ApprovalWitness.evaluate(input, &ctx(root)).expect("never errors")
    }

    /// O gesto de uma pergunta do catálogo, sem opção nenhuma oferecida.
    fn gesture(question: &str) -> &'static Gesture {
        gesture_of(question, &[], "").map(|(gesture, _)| gesture).expect("a gesture question")
    }

    /// O gesto de uma pergunta com as opções `options` e o cabeçalho `header`.
    fn gesture_asked(question: &str, options: &[&str], header: &str) -> Option<(&'static Gesture, String)> {
        let offered: Vec<String> = options.iter().map(|l| (*l).to_string()).collect();
        gesture_of(question, &offered, header)
    }

    /// Só o rótulo do catálogo, por inteiro, é a opção de aprovar.
    #[test]
    fn only_the_catalog_label_is_the_approve_option() {
        let approval = gesture(QUESTION);
        for yes in ["Aprovar", "Approve", " Aprovar "] {
            assert!(is_yes(approval, yes), "{yes}");
        }
        for no in ["Não aprovar", "Don't approve", "Aprovar depois", "APROVAR", "Desaprovar", "Ajustar", "Aceitar"] {
            assert!(!is_yes(approval, no), "{no}");
        }
    }

    /// Os gestos são uma lista: a aprovação da spec, reconhecida pela frase
    /// do catálogo, e a mudança que parte de um agente, reconhecida pelas
    /// duas opções dela, com o código no cabeçalho — a frase da pergunta
    /// dessa nunca é comparada. Sem as duas opções não há gesto de mudança, e
    /// o cabeçalho sem código deixa o gesto sem o que decidir.
    #[test]
    fn the_gestures_are_a_list_with_the_spec_approval_and_the_agent_change() {
        let yeses: Vec<&str> = GESTURES.iter().map(|g| g.yes).collect();
        assert_eq!(yeses, ["approval.option", "change.accept"]);

        let (approval, slot) = gesture_of("Approve this spec?", &[], "").expect("the approval, in English");
        assert!(matches!(approval.keyed, Keyed::Phrase("approval.question")));
        assert_eq!(slot, "");

        let options = ["Aceitar", "Recusar"];
        let (change, code) = gesture_asked("Posso seguir assim?", &options, "onda-3-a1b2c3").expect("the change");
        assert!(matches!(change.keyed, Keyed::Options("change.decline")));
        assert_eq!(code, "onda-3-a1b2c3", "o código vem do cabeçalho, não da frase");
        assert_eq!(
            gesture_asked("Whatever the words are", &["Accept", "Decline"], " onda-3-a1b2c3 ").map(|(_, c)| c),
            Some("onda-3-a1b2c3".to_string()),
            "as duas opções em inglês também"
        );
        assert!(is_yes(change, "Aceitar") && is_yes(change, "Accept"));
        assert!(!is_yes(change, "Recusar") && !is_yes(change, "Aprovar"));

        // Sem código no cabeçalho o gesto é o mesmo, sem o que decidir.
        assert_eq!(gesture_asked("Posso seguir?", &options, "Mudança").map(|(_, c)| c), Some(String::new()));
        for bad in ["onda-3-A1B2C3", "onda--a1b2c3", "onda-3-a1b2c", "onda-3-a1b2cg", "onda-3"] {
            assert_eq!(gesture_asked("Posso seguir?", &options, bad).map(|(_, c)| c), Some(String::new()), "{bad}");
        }
        // Sem as duas opções do catálogo, a pergunta não é gesto nenhum.
        for not_a_gesture in [&["Aceitar", "Depois"][..], &["Sim", "Não"][..], &["Aceitar"][..], &[][..]] {
            assert!(gesture_asked("Posso seguir?", not_a_gesture, "onda-3-a1b2c3").is_none(), "{not_a_gesture:?}");
        }
    }

    /// A pergunta da mudança que parte de um agente, como a rodada a manda
    /// fazer: em palavras, sem o código dentro dela.
    fn change_question(wave: u64, change: &str) -> String {
        translate("change.question", Locale::PtBr)
            .replace("{wave}", &wave.to_string())
            .replace("{change}", change)
    }

    /// As mensagens de usuário da spec `epic`, com a testemunha de cada uma.
    fn witnessed(root: &Path) -> Vec<(String, Option<Value>)> {
        DiskSpecState::new(root)
            .log("epic")
            .map(|log| {
                log.visible()
                    .into_iter()
                    .filter(|e| e.event_type == "message" && e.str_field("author") == Some("user"))
                    .map(|e| (e.str_field("text").unwrap_or_default().to_string(), e.fields.get("witness").cloned()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// O clique numa opção da pergunta da mudança é gravado com a
    /// testemunha, que leva o código da mudança ao lado da resposta, e a
    /// testemunha diz ao assistente o que o usuário escolheu. Texto livre é
    /// gravado sem testemunha e não aceita nada; sem spec atual, nada é
    /// gravado, e ela diz isso.
    #[test]
    fn a_change_click_is_recorded_with_its_witness_and_free_text_is_not() {
        if ambient_override() {
            return;
        }
        let dir = spec_with(&[json!({ "phase": "running", "branch": "feature/epic" })]);
        let root = dir.path();
        let code = "onda-2-abc123";
        let question = change_question(2, "A onda 2 precisa da 1 antes.");
        let options = ["Aceitar", "Recusar"];
        let ask = |answer: Value| ask_with(&question, code, &options, answer);

        let said = witness(root, &ask(json!("Aceitar")));
        let expected = say("change.witness.accepted", lang(root), &[("{code}", code)]);
        assert_eq!(said, Verdict::Inject { context: expected });

        let said = witness(root, &ask(json!("Recusar")));
        let expected = say("change.witness.declined", lang(root), &[("{code}", code)]);
        assert_eq!(said, Verdict::Inject { context: expected });

        match witness(root, &ask(json!("Aceitar, pode seguir"))) {
            Verdict::Inject { context } => {
                assert!(context.contains("\"Aceitar\", \"Recusar\""), "shows the menu: {context}");
            }
            other => panic!("free text is explained, got {other:?}"),
        }

        let clicked = |answer: &str| Some(json!({ "question": question, "answer": answer, "change": code }));
        assert_eq!(
            witnessed(root),
            [
                (format!("{question}\nAceitar"), clicked("Aceitar")),
                (format!("{question}\nRecusar"), clicked("Recusar")),
                (format!("{question}\nAceitar, pode seguir"), None),
            ]
        );

        let none = tempdir().unwrap();
        let said = witness(none.path(), &ask(json!("Aceitar")));
        let expected = say("change.witness.no_spec", lang(none.path()), &[("{code}", code)]);
        assert_eq!(said, Verdict::Inject { context: expected });
        assert!(!none.path().join(".claude").exists(), "no spec, nothing recorded");
    }

    /// O gesto se reconhece pelas opções da mudança, com a pergunta escrita
    /// com as palavras do usuário: o clique na opção do catálogo grava a
    /// testemunha com o código do cabeçalho ao lado da resposta. Sem código
    /// no cabeçalho nada é aceito, e a testemunha diz o que fazer.
    #[test]
    fn o_gesto_e_reconhecido_na_pergunta_escrita_com_as_palavras_do_usuario() {
        if ambient_override() {
            return;
        }
        let dir = spec_with(&[json!({ "phase": "running", "branch": "feature/epic" })]);
        let root = dir.path();
        let code = "onda-3-a1b2c3";
        let question = "A onda 3 travou e quer a onda 2 antes dela. Posso seguir assim?";
        let options = ["Aceitar", "Recusar"];

        let said = witness(root, &ask_with(question, code, &options, json!("Aceitar")));
        let expected = say("change.witness.accepted", lang(root), &[("{code}", code)]);
        assert_eq!(said, Verdict::Inject { context: expected });

        let clicked = Some(json!({ "question": question, "answer": "Aceitar", "change": code }));
        assert_eq!(witnessed(root), [(format!("{question}\nAceitar"), clicked)]);

        // A mesma pergunta sem o código no cabeçalho não aceita nada, e o
        // clique fica gravado sem código nenhum ao lado da resposta.
        let said = witness(root, &ask_with(question, "Mudança", &options, json!("Aceitar")));
        assert_eq!(said, Verdict::Inject { context: say("change.witness.no_code", lang(root), &[]) });
        let last = witnessed(root).pop().expect("o clique gravado");
        assert_eq!(last.1, Some(json!({ "question": question, "answer": "Aceitar" })), "sem código: {last:?}");

        match witness(root, &ask_with(question, code, &options, json!("Aceitar, obrigado"))) {
            Verdict::Inject { context } => {
                assert!(context.contains("\"Aceitar\", \"Recusar\""), "shows the menu: {context}");
            }
            other => panic!("free text is explained, got {other:?}"),
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
    /// vai ao assistente, a trava continua lendo a spec em plano e o arquivo
    /// de eventos fica byte por byte como estava. A recusa diz que nada foi
    /// gravado, e nada foi.
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
        bind_session_spec(&root.to_string_lossy(), SESSION, "epic");
        let file = store::spec_file(root, "epic").unwrap();
        let before = outside_messages(&file);
        match witness(root, &approve_or_adjust("Aprovar")) {
            Verdict::Inject { context } => assert!(context.contains("MSTD-POINT-0001"), "names the point: {context}"),
            other => panic!("the refusal is explained, got {other:?}"),
        }
        assert_eq!(outside_messages(&file), before, "the refusal wrote nothing but the answer");
        let lock = crate::shared::spec_state::lock_state(root, "epic").expect("the spec has its event file");
        assert_eq!(lock.phase, Some("plan"), "the lock still reads plan");
        assert!(!lock.approved, "nothing approved");
        assert!(
            mustard_core::domain::spec_state::birth_event(&DiskSpecState::new(root).log("epic").unwrap()).is_none(),
            "no state was born by the refused approval"
        );
    }

    /// Um arquivo de eventos sem nenhum `state` e sem `meta.json` é uma spec
    /// em plano, sem nascimento: o "Aprovar" grava a aprovação, e só ela.
    #[test]
    fn a_spec_file_without_a_state_is_born_and_approved() {
        if ambient_override() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        record_for(root, "epic", "message", json!({ "author": "user", "text": "oi" }));
        bind_session_spec(&root.to_string_lossy(), SESSION, "epic");
        witness(root, &approve_or_adjust("Aprovar"));
        assert!(state(root).approved, "the spec with no phase is approved");
        let log = std::fs::read_to_string(store::spec_file(root, "epic").unwrap()).unwrap();
        let phases: Vec<String> =
            log.lines().filter_map(|l| serde_json::from_str::<Value>(l).unwrap()["phase"].as_str().map(str::to_string)).collect();
        assert_eq!(phases, ["approved"], "one single write, {log}");
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
        bind_session_spec(&root.to_string_lossy(), SESSION, "ajuste");

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
        bind_session_spec(&root.to_string_lossy(), SESSION, "outra");

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
        bind_session_spec(&root.to_string_lossy(), SESSION, "epic");

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
        bind_session_spec(&root.to_string_lossy(), SESSION, "epic");

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

    /// As mensagens de usuário gravadas na spec `epic`.
    fn user_messages(root: &Path) -> Vec<String> {
        DiskSpecState::new(root)
            .log("epic")
            .map(|log| {
                log.visible()
                    .into_iter()
                    .filter(|e| e.event_type == "message" && e.str_field("author") == Some("user"))
                    .filter_map(|e| e.str_field("text").map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A resposta de cada pergunta do levantamento vai para a conversa, com a
    /// pergunta e a nota; a resposta digitada também. A de aprovação vai do
    /// mesmo jeito, ao lado do estado aprovado. Uma pergunta cancelada e uma
    /// resposta sem spec atual não gravam nada.
    #[test]
    fn each_answered_question_is_recorded_as_a_user_message() {
        if ambient_override() {
            return;
        }
        let dir = in_plan();
        let root = dir.path();
        let mut survey = ask_on("Qual o objetivo?", &["Enxugar", "Crescer"], json!("Enxugar"));
        survey.raw["tool_response"]["annotations"] = json!({ "Qual o objetivo?": { "notes": "sem peça nova" } });
        assert_eq!(witness(root, &survey), Verdict::Allow, "another question never decides");
        witness(root, &ask_on("Quais telas?", &["Login", "Painel"], json!(["Login", "Painel"])));
        witness(root, &ask_on("Qual a base?", &["dev"], json!("a main, por favor")));
        witness(root, &ask_on("Cancelada?", &["Sim"], json!({})));
        witness(root, &approve_or_adjust("Aprovar"));
        assert!(state(root).approved);
        assert_eq!(
            user_messages(root),
            [
                "Qual o objetivo?\nEnxugar\nsem peça nova",
                "Quais telas?\nLogin, Painel",
                "Qual a base?\na main, por favor",
                "Aprovar esta spec?\nAprovar",
            ]
        );

        let bare = tempdir().unwrap();
        witness(bare.path(), &ask_on("Qual o objetivo?", &["Enxugar"], json!("Enxugar")));
        assert!(!bare.path().join(".claude").exists(), "no spec, nothing recorded");
    }
}
