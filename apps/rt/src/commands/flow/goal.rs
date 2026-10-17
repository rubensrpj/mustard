//! Goal confirmation is reconstructed from native question witnesses, never
//! inferred from a free-standing “yes” or kept only in model memory.
use mustard_core::{
    domain::{
        spec_events::{SpecEvent, SpecLog},
        survey,
    },
    platform::i18n::Locale,
};
use serde_json::{Value, json};

pub(crate) fn question(log: &SpecLog, lang: Locale) -> Option<String> {
    let goal = survey::goal(log)?.str_field("text")?;
    Some(match lang {
        Locale::PtBr => format!("O objetivo é: {goal}\nConfirma este objetivo?"),
        Locale::EnUs => format!("The goal is: {goal}\nConfirm this goal?"),
    })
}
pub(crate) fn labels(lang: Locale) -> (&'static str, &'static str) {
    match lang {
        Locale::PtBr => ("Confirmar objetivo", "Ajustar objetivo"),
        Locale::EnUs => ("Confirm goal", "Adjust goal"),
    }
}

/// Como pedir a confirmação sem repetir o objetivo dentro da pergunta.
fn hint(lang: Locale) -> String {
    let yes = labels(lang).0;
    match lang {
        Locale::PtBr => format!(
            "Mostre o objetivo inteiro no chat. Logo depois, faça uma pergunta curta, com o cabeçalho MustardGoal e as duas opções. O clique em \"{yes}\" vale quando a sua última fala antes da pergunta traz o objetivo inteiro. A pergunta com o objetivo dentro também vale."
        ),
        Locale::EnUs => format!(
            "Show the whole goal in the chat. Right after, ask a short question, with the MustardGoal header and both options. The \"{yes}\" click counts when your last message before the question carries the whole goal. The question with the goal inside also counts."
        ),
    }
}

/// O texto sem diferença de espaços: cada sequência de espaços e quebras de
/// linha vira um espaço só.
fn spaced(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A última resposta do assistente antes do evento `id` traz o objetivo
/// inteiro, sem diferença de espaços.
fn shown_before(log: &SpecLog, id: u64) -> bool {
    let Some(goal) = survey::goal(log).and_then(|goal| goal.str_field("text")).map(spaced) else {
        return false;
    };
    !goal.is_empty()
        && log
            .visible()
            .into_iter()
            .rev()
            .find(|event| event.event_type == "response" && event.id < id)
            .and_then(|event| event.str_field("text"))
            .is_some_and(|text| spaced(text).contains(&goal))
}

/// A mensagem `event` confirma o objetivo: é do usuário, com a testemunha do
/// clique em "Confirmar objetivo" (ou "Confirm goal"). A pergunta respondida
/// traz o objetivo inteiro, ou é curta e a última resposta do assistente
/// antes dela mostrou o objetivo inteiro. Palavra solta em texto livre não
/// tem testemunha e nunca conta.
pub(crate) fn confirms(log: &SpecLog, event: &SpecEvent) -> bool {
    if event.event_type != "message" || event.str_field("author") != Some("user") {
        return false;
    }
    let Some(witness) = event.fields.get("witness") else {
        return false;
    };
    [Locale::PtBr, Locale::EnUs].into_iter().any(|lang| {
        witness["answer"] == labels(lang).0
            && (question(log, lang).is_some_and(|question| witness["question"] == question)
                || shown_before(log, event.id))
    })
}

pub(crate) fn status(log: &SpecLog, lang: Locale) -> Value {
    let Some(goal) = survey::goal(log) else {
        return json!({"confirmed":false,"reason":"goal-missing"});
    };
    let question = question(log, lang).unwrap_or_default();
    let (yes, no) = labels(lang);
    let witness = log.visible().into_iter().find(|event| confirms(log, event));
    // O objetivo escrito pelo próprio usuário já está autorizado. A frase
    // sugerida aceita com um "sim" pede a testemunha do clique.
    let direct = goal
        .int("origin")
        .and_then(|id| log.get(id))
        .is_some_and(|event| {
            event.event_type == "message"
                && event.str_field("author") == Some("user")
                && event.str_field("text").is_some_and(|text| {
                    text.trim()
                        .starts_with(goal.str_field("text").unwrap_or_default().trim())
                })
        });
    let approved_spec = log.visible().into_iter().find(|event| {
        event.event_type == "state"
            && event.id > goal.id
            && event.str_field("phase") == Some("approved")
            && event.str_field("author") == Some("user")
            && event.fields.contains_key("witness")
    });
    let mut status = json!({"confirmed":direct||witness.is_some()||approved_spec.is_some(),"goal":goal.id,"message":witness.map(|event|event.id),
        "source":if direct {"user-objective"} else if approved_spec.is_some() {"approved-spec"} else {"native-question-witness"}});
    if !direct && witness.is_none() && approved_spec.is_none() {
        status["question"] = json!(question);
        status["header"] = json!("MustardGoal");
        status["options"] = json!([yes, no]);
        status["hint"] = json!(hint(lang));
    }
    status
}
