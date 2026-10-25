//! Goal confirmation is reconstructed from native question witnesses, never
//! inferred from a free-standing “yes” or kept only in model memory.
use mustard_core::{
    domain::{
        lessons::comparable,
        spec_events::{SpecEvent, SpecLog},
        survey,
        text::{fold, words},
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

/// O texto como a fala e o objetivo se comparam: só as palavras, em
/// minúsculas e sem acento, com um espaço entre elas e outro em cada ponta.
/// Negrito, crase, aspas, vírgula, ponto e marca de lista ficam de fora, e as
/// pontas fazem a busca casar só palavra inteira.
fn plain(text: &str) -> String {
    let folded = fold(text);
    format!(" {} ", words(&folded).collect::<Vec<_>>().join(" "))
}

/// A última resposta do assistente antes do evento `id` traz o objetivo
/// inteiro.
///
/// A comparação é só das palavras ([`plain`]), sem acento, maiúscula,
/// formatação nem pontuação. Letra por letra, ela quebrava por um acento: o
/// `write` aceita um objetivo gravado sem acento, a fala acentuada não casava,
/// e o clique do usuário em "Confirmar objetivo" se perdia calado (campo,
/// 21/10/2026). Depois, quebrava pelo negrito, pela crase e por uma vírgula
/// que o assistente punha ao mostrar o objetivo no chat.
fn shown_before(log: &SpecLog, id: u64) -> bool {
    let Some(goal) = survey::goal(log).and_then(|goal| goal.str_field("text")).map(plain) else {
        return false;
    };
    !goal.trim().is_empty()
        && log
            .visible()
            .into_iter()
            .rev()
            .find(|event| event.event_type == "response" && event.id < id)
            .and_then(|event| event.str_field("text"))
            .is_some_and(|text| plain(text).contains(&goal))
}

/// A testemunha do evento diz que o usuário escolheu o "sim" da pergunta do
/// objetivo. Compara pela forma de [`comparable`], sem acento nem maiúscula.
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

/// Por que o clique que veio não valeu, com o objetivo `goal` como está
/// gravado. Vem ANTES da dica de sempre, nunca no lugar dela: o motivo diz o
/// que aconteceu, e a dica segue dizendo como perguntar de novo.
fn differs_hint(lang: Locale, goal: &str) -> String {
    let yes = labels(lang).0;
    match lang {
        Locale::PtBr => format!(
            "O usuário já escolheu \"{yes}\", e o clique não valeu: nem a pergunta respondida nem a sua fala antes dela traziam o objetivo gravado. A fala pode mudar acento, maiúsculas, formatação e pontuação, não as palavras. O objetivo gravado é: {goal}\nGrave uma versão nova do objetivo com o texto que você mostrou, ou mostre este. {}",
            hint(lang)
        ),
        Locale::EnUs => format!(
            "The user already chose \"{yes}\" and the click did not count: neither the question answered nor your message before it carried the recorded goal. Your message may change accents, case, formatting and punctuation, not the words. The recorded goal is: {goal}\nRecord a new version of the goal with the text you showed, or show this one. {}",
            hint(lang)
        ),
    }
}

/// O que dizer, no próprio clique, quando o usuário escolheu "Confirmar
/// objetivo" e o clique não confirmou: o motivo, o objetivo gravado e como
/// perguntar de novo. Sem isso, o gancho do clique saía calado e o aviso só
/// chegava no comando seguinte. `None` sem objetivo gravado.
pub(crate) fn refused_click(log: &SpecLog, lang: Locale) -> Option<String> {
    let goal = survey::goal(log)?.str_field("text")?;
    Some(differs_hint(lang, goal))
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
            // O objetivo como está gravado vai junto: sem ele, quem lê a
            // resposta tem de ir buscar o texto para saber o que mostrar, e
            // foi mostrando outro texto que o clique se perdeu.
            if let Some(text) = goal.str_field("text") {
                status["hint"] = json!(differs_hint(lang, text));
                status["goal_text"] = json!(text);
            }
        }
    }
    status
}
