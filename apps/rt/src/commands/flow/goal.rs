//! Goal confirmation is reconstructed from native question witnesses, never
//! inferred from a free-standing “yes” or kept only in model memory.
use mustard_core::{
    domain::{
        lessons::comparable,
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

/// A última resposta do assistente antes do evento `id` traz o objetivo
/// inteiro.
///
/// A comparação é a de [`comparable`]: minúsculas, sem acento e com um espaço
/// só entre as palavras — a mesma com que o banco de lições acha lição
/// repetida. Letra por letra, ela quebrava por um acento. O estilo de resposta
/// do projeto manda escrever em português correto, o `write` aceita um
/// objetivo gravado sem acento, e então a fala acentuada não casava com o
/// texto gravado: o clique do usuário em "Confirmar objetivo" se perdia
/// calado, e a retomada seguia pedindo a confirmação (campo, 21/10/2026).
fn shown_before(log: &SpecLog, id: u64) -> bool {
    let Some(goal) = survey::goal(log).and_then(|goal| goal.str_field("text")).map(comparable) else {
        return false;
    };
    !goal.is_empty()
        && log
            .visible()
            .into_iter()
            .rev()
            .find(|event| event.event_type == "response" && event.id < id)
            .and_then(|event| event.str_field("text"))
            .is_some_and(|text| comparable(text).contains(&goal))
}

/// A testemunha do evento diz que o usuário escolheu o "sim" da pergunta do
/// objetivo. Compara pela forma de [`comparable`], pelo mesmo motivo de
/// [`shown_before`].
fn answered_yes(event: &SpecEvent, lang: Locale) -> bool {
    event
        .fields
        .get("witness")
        .and_then(|witness| witness["answer"].as_str())
        .is_some_and(|said| comparable(said) == comparable(labels(lang).0))
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
        answered_yes(event, lang)
            && (question(log, lang).is_some_and(|asked| {
                witness["question"].as_str().is_some_and(|said| comparable(said) == comparable(&asked))
            }) || shown_before(log, event.id))
    })
}

/// A mensagem do usuário que escolheu "Confirmar objetivo" e não confirmou: a
/// testemunha do clique está lá, e nem a pergunta respondida nem a fala antes
/// dela traziam o objetivo gravado.
///
/// É ela que separa "o usuário ainda não respondeu" de "o usuário respondeu e
/// o Mustard perdeu a resposta". Sem essa separação, a retomada repetia a
/// mesma pergunta sem dizer por quê, e o usuário clicava de novo no mesmo
/// botão (campo, 21/10/2026).
fn clicked_without_match(log: &SpecLog, lang: Locale) -> Option<&SpecEvent> {
    log.visible().into_iter().rev().find(|event| {
        event.event_type == "message"
            && event.str_field("author").map(str::trim) == Some("user")
            && answered_yes(event, lang)
            && !confirms(log, event)
    })
}

/// Por que o clique que veio não valeu. Vem ANTES da dica de sempre, nunca no
/// lugar dela: o motivo diz o que aconteceu, e a dica segue dizendo como
/// perguntar de novo.
fn differs_hint(lang: Locale) -> String {
    let yes = labels(lang).0;
    match lang {
        Locale::PtBr => format!(
            "O usuário já escolheu \"{yes}\", e o clique não valeu: nem a pergunta respondida nem a sua fala antes dela traziam o objetivo como ele está gravado, fora diferença de acento e de maiúsculas. Grave uma versão nova do objetivo com o texto que você mostrou, ou mostre o objetivo exatamente como `goal` o traz. {}",
            hint(lang)
        ),
        Locale::EnUs => format!(
            "The user already chose \"{yes}\" and the click did not count: neither the question answered nor your message before it carried the goal as it is recorded, accents and case aside. Record a new version of the goal with the text you showed, or show the goal exactly as `goal` gives it. {}",
            hint(lang)
        ),
    }
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
        // O clique que veio e não valeu é dito, com o número da mensagem: a
        // pergunta repetida sem motivo fazia o usuário clicar duas vezes no
        // mesmo botão.
        if let Some(clicked) = clicked_without_match(log, lang) {
            status["reason"] = json!("goal-text-differs");
            status["clicked"] = json!(clicked.id);
            status["hint"] = json!(differs_hint(lang));
            // O objetivo como está gravado vai junto: sem ele, quem lê a
            // resposta tem de ir buscar o texto para saber o que mostrar, e
            // foi mostrando outro texto que o clique se perdeu.
            if let Some(text) = goal.str_field("text") {
                status["goal_text"] = json!(text);
            }
        }
    }
    status
}
