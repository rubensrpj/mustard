//! Goal confirmation is reconstructed from native question witnesses, never
//! inferred from a free-standing “yes” or kept only in model memory.
use mustard_core::{
    domain::{spec_events::SpecLog, survey},
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
pub(crate) fn status(log: &SpecLog, lang: Locale) -> Value {
    let Some(goal) = survey::goal(log) else {
        return json!({"confirmed":false,"reason":"goal-missing"});
    };
    let question = question(log, lang).unwrap_or_default();
    let (yes, no) = labels(lang);
    let witness = log.visible().into_iter().find(|event| {
        event.event_type == "message"
            && event.str_field("author") == Some("user")
            && event.fields.get("witness").is_some_and(|w| {
                [Locale::PtBr, Locale::EnUs].into_iter().any(|lang| {
                    self::question(log, lang).is_some_and(|question| {
                        w["question"] == question && w["answer"] == labels(lang).0
                    })
                })
            })
    });
    // A goal directly written by the user is already authorized. A suggested
    // sentence accepted with “yes” must have the exact native witness.
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
    }
    status
}
