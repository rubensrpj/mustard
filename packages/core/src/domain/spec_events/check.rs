//! A conferência de um evento sozinho: o rascunho de quem grava fica pronto
//! para a conferência, e cada campo é conferido pelo tipo, pela forma e pela
//! situação. O que depende do resto do arquivo mora na conferência contra o
//! arquivo.

use serde_json::{Map, Value};

use super::{
    opt, req, type_spec, Field, Kind, Refusal, TypeSpec, AUTHORS, BINARY_FIELDS, DEFAULT_AUTHOR, DELIVERED_MAX_CHARS,
    PURGED_FIELD, REFUSED_FIELDS,
};

/// `true` para o que não vale como valor: ausente, `null`, texto em branco,
/// lista vazia ou objeto vazio. Número e `true`/`false` nunca são vazios.
pub(crate) fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        Value::Bool(_) | Value::Number(_) => false,
    }
}

/// O rascunho de quem grava, pronto para a conferência: sem os campos que só
/// o binário escreve, com o tipo pedido e com o autor (o assistente, quando
/// quem grava não diz). Os campos de [`REFUSED_FIELDS`] ficam, para que
/// [`validate`] recuse o evento que os trouxe.
#[must_use]
pub fn normalize(mut draft: Map<String, Value>, event_type: &str) -> Map<String, Value> {
    for field in BINARY_FIELDS.iter().filter(|f| !REFUSED_FIELDS.contains(f)) {
        draft.remove(*field);
    }
    draft.remove(PURGED_FIELD);
    draft.insert("type".into(), Value::String(event_type.trim().to_string()));
    if draft.get("author").is_none_or(is_empty) {
        draft.insert("author".into(), Value::String(DEFAULT_AUTHOR.into()));
    }
    draft
}

/// Confere um evento sozinho: o tipo existe, o autor é conhecido, cada campo
/// obrigatório está preenchido e cada campo tem a forma certa. O que depende
/// do resto do arquivo (o número apontado existe, o arquivo citado existe)
/// fica para [`check_against`](super::check_against) e para o gravador.
pub fn validate(event: &Map<String, Value>) -> Result<(), Refusal> {
    let found = event.get("type").and_then(Value::as_str).unwrap_or_default();
    let Some(spec) = type_spec(found) else {
        return Err(Refusal::UnknownType { found: found.to_string() });
    };
    if let Some(field) = REFUSED_FIELDS.iter().find(|f| event.contains_key(**f)) {
        return Err(Refusal::BinaryOnlyField { field: (*field).to_string() });
    }
    check_field(event, spec.name, req("author", Kind::OneOf(AUTHORS)))?;
    // O `origin` é obrigatório no que o assistente grava a partir da
    // conversa; o que o binário grava, como os critérios tirados do
    // `spec.md`, não tem mensagem de onde veio.
    let by_assistant = event.get("author").and_then(Value::as_str) == Some(DEFAULT_AUTHOR);
    check_field(
        event,
        spec.name,
        Field { name: "origin", kind: Kind::Int, required: spec.needs_origin && by_assistant },
    )?;
    for envelope in [opt("label", Kind::Text), opt("replaces", Kind::Ref)] {
        check_field(event, spec.name, envelope)?;
    }
    for shared in [opt("text", Kind::Text), opt("keys", Kind::Texts)] {
        if !spec.fields.iter().any(|f| f.name == shared.name) {
            check_field(event, spec.name, shared)?;
        }
    }
    for field in spec.fields {
        check_field(event, spec.name, *field)?;
    }
    // O campo que o tipo não declara é recusado pelo nome: ele entraria
    // calado e ficaria gravado sem ninguém ver, e um nome escrito errado
    // nunca mais seria lido por nada.
    if let Some(found) = event.keys().find(|key| !accepts_field(spec, key)) {
        return Err(Refusal::UnknownField {
            event_type: spec.name.to_string(),
            field: found.clone(),
            accepted: accepted_fields(spec),
        });
    }
    check_nested(event, spec.name)?;
    check_conditions(event, spec.name)?;
    check_fact_sources(event, spec.name)
}

/// Os campos que toda linha pode trazer, fora os do tipo: o envelope, o campo
/// de busca, a marca do expurgo, o rótulo, a versão nova de um item e a
/// mensagem de onde ele veio.
const COMMON_FIELDS: &[&str] =
    &["v", "id", "code", "at", "type", "author", "search", "purged", "label", "replaces", "origin", "text", "keys"];

/// O tipo aceita este campo? Aceita os comuns a toda linha e os que ele
/// declara.
fn accepts_field(spec: &TypeSpec, name: &str) -> bool {
    COMMON_FIELDS.contains(&name) || spec.fields.iter().any(|field| field.name == name)
}

/// Os campos que um tipo aceita, separados por vírgula: os que ele declara,
/// depois os comuns a toda linha.
fn accepted_fields(spec: &TypeSpec) -> String {
    let own: Vec<&str> = spec.fields.iter().map(|field| field.name).collect();
    let rest: Vec<&str> = COMMON_FIELDS.iter().copied().filter(|name| !own.contains(name)).collect();
    own.into_iter().chain(rest).collect::<Vec<_>>().join(", ")
}

pub(crate) fn check_field(event: &Map<String, Value>, event_type: &str, field: Field) -> Result<(), Refusal> {
    match event.get(field.name) {
        Some(value) if !is_empty(value) => {
            if field.kind.accepts(value) {
                Ok(())
            } else {
                Err(Refusal::InvalidValue {
                    event_type: event_type.to_string(),
                    field: field.name.to_string(),
                    expected: field.kind,
                })
            }
        }
        _ if field.required => Err(missing(event_type, field.name)),
        _ => Ok(()),
    }
}

pub(super) fn missing(event_type: &str, field: &str) -> Refusal {
    Refusal::MissingField { event_type: event_type.to_string(), field: field.to_string() }
}

/// Os objetos com campos obrigatórios próprios, dentro de uma lista ou de um
/// campo: o tipo, o campo e os campos de dentro. A fonte de cada fato do ponto
/// tem recusa própria e não entra aqui.
const NESTED: &[(&str, &str, &[&str])] = &[
    ("point", "facts", &["text"]),
    ("task", "files", &["path"]),
    ("skill", "examples", &["path", "why"]),
    ("send", "skills", &["name", "sha"]),
    ("verdict", "criteria", &["criterion", "tests_rule"]),
    ("verdict", "lessons", &["lesson", "repeated"]),
    ("state", "witness", &["question", "answer"]),
    ("message", "witness", &["question", "answer"]),
    ("remove", "filter", &["type", "from", "to"]),
];

fn check_nested(event: &Map<String, Value>, event_type: &str) -> Result<(), Refusal> {
    for (owner, field, inner) in NESTED {
        if *owner != event_type {
            continue;
        }
        match event.get(*field) {
            Some(Value::Array(items)) => {
                for (i, item) in items.iter().enumerate() {
                    let Some(obj) = item.as_object() else { continue };
                    for key in *inner {
                        if obj.get(*key).is_none_or(is_empty) {
                            return Err(missing(event_type, &format!("{field}[{}].{key}", i + 1)));
                        }
                    }
                }
            }
            Some(Value::Object(obj)) => {
                for key in *inner {
                    if obj.get(*key).is_none_or(is_empty) {
                        return Err(missing(event_type, &format!("{field}.{key}")));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Os campos que só são obrigatórios numa situação: a testemunha na
/// aprovação, o motivo no descarte, o endereço da publicação que deu certo, a
/// fonte dos fatos do ponto aberto, os exemplos da skill que nasce, o alvo da
/// remoção, os critérios da revisão de uma onda. O ponto que fecha outro
/// (`closes`) nunca fica aberto, e o que "não se aplica" leva o motivo.
fn check_conditions(event: &Map<String, Value>, event_type: &str) -> Result<(), Refusal> {
    let has = |f: &str| event.get(f).is_some_and(|v| !is_empty(v));
    let need = |f: &str| if has(f) { Ok(()) } else { Err(missing(event_type, f)) };
    let word = |f: &str| event.get(f).and_then(Value::as_str).unwrap_or_default();
    match event_type {
        "state" => match word("phase") {
            "approved" => need("witness"),
            "discarded" => need("reason"),
            "pr_open" => need("pr"),
            _ => Ok(()),
        },
        "publish" => {
            if event.get("ok").and_then(Value::as_bool) == Some(true) {
                need("url")
            } else {
                need("reason")
            }
        }
        // A revisão de uma onda diz quais critérios conferiu. A revisão final
        // do conjunto não confere critério: ela confere o encaixe das ondas, e
        // cobrar o campo dela travava o fechamento da spec de duas ondas ou
        // mais, que não fecha sem essa revisão.
        "verdict" => {
            if event.get("final").and_then(Value::as_bool) == Some(true) {
                Ok(())
            } else {
                need("criteria")
            }
        }
        "point" => {
            let reminders = event.get("reminders").and_then(Value::as_array).map_or(0, Vec::len);
            if reminders > 3 {
                return Err(Refusal::WrongCount {
                    event_type: event_type.to_string(),
                    field: "reminders".into(),
                    min: 0,
                    max: 3,
                    count: reminders,
                });
            }
            let status = word("status");
            if status == "open" {
                if has("closes") {
                    return Err(Refusal::ClosingPointOpen);
                }
                return need("facts");
            }
            // A versão nova de um ponto pode vir sem `closes`: o binário copia
            // o da versão antiga e confere de novo (veja
            // [`carry_closed_identity`]).
            if !has("replaces") {
                need("closes")?;
            }
            if status == "not_applicable" {
                return if has("reason") { Ok(()) } else { Err(Refusal::NotApplicableNeedsReason) };
            }
            if has("result") || has("reason") { Ok(()) } else { Err(missing(event_type, "result")) }
        }
        // O entregou volta para a janela principal a cada onda: ele conta o
        // que mudou, e não repete o pedido.
        "delivered" => {
            let chars = word("text").chars().count();
            if chars > DELIVERED_MAX_CHARS {
                return Err(Refusal::DeliveredTooLong { chars, max: DELIVERED_MAX_CHARS });
            }
            Ok(())
        }
        "skill" if word("action") == "create" => {
            need("examples")?;
            let count = event.get("examples").and_then(Value::as_array).map_or(0, Vec::len);
            if (2..=3).contains(&count) {
                Ok(())
            } else {
                Err(Refusal::WrongCount {
                    event_type: event_type.to_string(),
                    field: "examples".into(),
                    min: 2,
                    max: 3,
                    count,
                })
            }
        }
        "remove" => {
            if !has("targets") && !has("filter") {
                return Err(missing(event_type, "targets"));
            }
            let Some(filter) = event.get("filter").and_then(Value::as_object) else {
                return Ok(());
            };
            let filtered = filter.get("type").and_then(Value::as_str).unwrap_or_default();
            if type_spec(filtered).is_none() {
                return Err(Refusal::UnknownType { found: filtered.to_string() });
            }
            for bound in ["from", "to"] {
                if !filter.get(bound).is_some_and(|v| Kind::Time.accepts(v)) {
                    return Err(Refusal::InvalidValue {
                        event_type: event_type.to_string(),
                        field: format!("filter.{bound}"),
                        expected: Kind::Time,
                    });
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Todo fato de um ponto leva a fonte: o arquivo e a linha, o comando com o
/// resultado, ou o número da mensagem do usuário.
fn check_fact_sources(event: &Map<String, Value>, event_type: &str) -> Result<(), Refusal> {
    if event_type != "point" {
        return Ok(());
    }
    let facts = event.get("facts").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    for (i, fact) in facts.iter().enumerate() {
        if fact.get("source").is_none_or(is_empty) {
            return Err(Refusal::FactWithoutSource { fact: i + 1 });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::spec_events::tests::checked;
    use crate::platform::i18n::Locale;

    /// O entregou volta para a janela principal a cada onda, e por isso tem
    /// teto: no limite passa, um caractere acima é recusado com o tamanho que
    /// ele tem, nos dois idiomas.
    #[test]
    fn a_delivery_note_over_the_character_cap_is_refused_with_its_size() {
        let note = |chars: usize| {
            json!({"author": "wave", "wave": 1, "text": "á".repeat(chars), "files": ["src/a.rs"]})
        };
        assert!(checked("delivered", note(DELIVERED_MAX_CHARS)).is_ok());
        let refusal = checked("delivered", note(DELIVERED_MAX_CHARS + 1)).unwrap_err();
        assert_eq!(refusal.reason(), "delivered-too-long");
        for lang in [Locale::PtBr, Locale::EnUs] {
            let message = refusal.message(lang);
            assert!(message.contains("8001"), "{message}");
            assert!(message.contains("8000"), "{message}");
        }
    }

    #[test]
    fn an_unknown_type_is_refused_by_name() {
        let refusal = checked("licao", json!({"text": "x"})).unwrap_err();
        assert_eq!(refusal, Refusal::UnknownType { found: "licao".into() });
        assert!(refusal.message(Locale::PtBr).contains("O tipo licao não existe"));
        assert!(refusal.message(Locale::EnUs).contains("no licao event type"));
    }

    #[test]
    fn a_missing_or_blank_required_field_is_refused_by_name() {
        let base = json!({"text": "Só o binário grava.", "keys": ["gravação"], "origin": 3});
        assert_eq!(
            checked("rule", base.clone()).unwrap_err(),
            Refusal::MissingField { event_type: "rule".into(), field: "example".into() }
        );
        let mut blank = base;
        blank["example"] = json!("   ");
        assert_eq!(
            checked("rule", blank).unwrap_err(),
            Refusal::MissingField { event_type: "rule".into(), field: "example".into() }
        );
        assert_eq!(
            checked("note", json!({"text": "t", "keys": [], "origin": 1})).unwrap_err(),
            Refusal::MissingField { event_type: "note".into(), field: "keys".into() }
        );
    }

    #[test]
    fn what_the_assistant_writes_from_the_conversation_needs_its_origin() {
        assert_eq!(
            checked("note", json!({"text": "t", "keys": ["k"]})).unwrap_err(),
            Refusal::MissingField { event_type: "note".into(), field: "origin".into() }
        );
        assert!(checked("message", json!({"text": "oi", "author": "user"})).is_ok());
    }

    /// O que o binário grava não tem mensagem de origem: um critério tirado do
    /// `spec.md` entra sem `origin`, e o mesmo critério pelo assistente, não.
    #[test]
    fn what_the_binary_writes_needs_no_origin() {
        let criterion = json!({"when": "w", "then": "t", "proof": "cargo test"});
        let mut by_binary = criterion.clone();
        by_binary["author"] = json!("binary");
        assert!(checked("criterion", by_binary).is_ok());
        assert_eq!(
            checked("criterion", criterion).unwrap_err(),
            Refusal::MissingField { event_type: "criterion".into(), field: "origin".into() }
        );
    }

    /// Uma tarefa que não cita arquivo entra sem o campo; quando o campo vem,
    /// cada arquivo continua precisando do caminho.
    #[test]
    fn a_task_without_files_is_accepted_and_a_file_without_path_is_not() {
        assert!(checked("task", json!({"wave": 1, "text": "Medir de novo.", "origin": 1})).is_ok());
        assert_eq!(
            checked("task", json!({"wave": 1, "text": "t", "files": [{"new": true}], "origin": 1})).unwrap_err(),
            Refusal::MissingField { event_type: "task".into(), field: "files[1].path".into() }
        );
    }

    #[test]
    fn situations_that_require_a_field() {
        let approved = json!({"phase": "approved", "author": "binary"});
        assert_eq!(
            checked("state", approved).unwrap_err(),
            Refusal::MissingField { event_type: "state".into(), field: "witness".into() }
        );
        let witness = json!({"phase": "approved", "author": "binary", "witness": {"question": "Aprovar?"}});
        assert_eq!(
            checked("state", witness).unwrap_err(),
            Refusal::MissingField { event_type: "state".into(), field: "witness.answer".into() }
        );
        let born = json!({"name": "s", "action": "create", "text": "t", "sha": "1",
            "examples": [{"path": "a.rs", "why": "w"}]});
        assert!(matches!(
            checked("skill", born).unwrap_err(),
            Refusal::WrongCount { min: 2, max: 3, count: 1, .. }
        ));
        let nothing = json!({"reason": "engano"});
        assert_eq!(
            checked("remove", nothing).unwrap_err(),
            Refusal::MissingField { event_type: "remove".into(), field: "targets".into() }
        );
        let bad_time = json!({"reason": "r", "filter": {"type": "message", "from": "21:03", "to": "21:10"}});
        assert!(matches!(
            checked("remove", bad_time).unwrap_err(),
            Refusal::InvalidValue { ref field, .. } if field == "filter.from"
        ));
    }

    /// A revisão de uma onda diz quais critérios conferiu, e a que não diz é
    /// recusada pelo campo. A revisão final do conjunto confere o encaixe das
    /// ondas, e não critério: ela entra sem o campo, aprovada ou reprovada, e
    /// a que traz critérios continua conferida item a item.
    #[test]
    fn only_the_final_review_of_the_whole_is_recorded_without_criteria() {
        let wave = json!({"author": "review", "wave": 1, "result": "approved", "text": "passou"});
        assert_eq!(
            checked("verdict", wave.clone()).unwrap_err(),
            Refusal::MissingField { event_type: "verdict".into(), field: "criteria".into() }
        );
        let mut judged = wave.clone();
        judged["criteria"] = json!([{"criterion": 7, "tests_rule": true}]);
        assert_eq!(checked("verdict", judged), Ok(()));

        for result in ["approved", "rejected"] {
            let mut whole = wave.clone();
            whole["result"] = json!(result);
            whole["final"] = json!(true);
            assert_eq!(checked("verdict", whole), Ok(()), "a revisão final {result} entra sem critérios");
        }
        let mut half = wave;
        half["final"] = json!(true);
        half["criteria"] = json!([{"criterion": 7}]);
        assert_eq!(
            checked("verdict", half).unwrap_err(),
            Refusal::MissingField { event_type: "verdict".into(), field: "criteria[1].tests_rule".into() }
        );
    }

    /// O código não vem de quem grava: o evento que o traz é recusado, com a
    /// mensagem nos dois idiomas. Os outros campos do binário continuam
    /// descartados.
    #[test]
    fn a_code_sent_by_the_caller_is_refused() {
        let draft = json!({"text": "t", "keys": ["k"], "origin": 1, "code": "MSTD-NOTE-0001"});
        let refusal = checked("note", draft).unwrap_err();
        assert_eq!(refusal, Refusal::BinaryOnlyField { field: "code".into() });
        let (pt, en) = (refusal.message(Locale::PtBr), refusal.message(Locale::EnUs));
        assert!(pt.contains("O campo code é gravado só pelo binário"), "{pt}");
        assert!(en.contains("The code field is written only by the binary"), "{en}");
        assert!(checked("note", json!({"text": "t", "keys": ["k"], "origin": 1, "id": 9, "at": "x"})).is_ok());
    }

    /// O ponto que fecha outro nunca fica aberto; o que "não se aplica" leva
    /// o motivo; o `closes` aceita o número ou o código do ponto.
    #[test]
    fn a_closing_point_is_never_open_and_not_applicable_takes_a_reason() {
        let base = json!({"block": "limits", "gap": "g", "from": "gap", "origin": 1, "closes": 3});
        let mut open = base.clone();
        open["status"] = json!("open");
        open["facts"] = json!([{"text": "f", "source": "mensagem 1"}]);
        assert_eq!(checked("point", open), Err(Refusal::ClosingPointOpen));

        let mut skipped = base.clone();
        skipped["status"] = json!("not_applicable");
        skipped["result"] = json!([1]);
        assert_eq!(checked("point", skipped.clone()), Err(Refusal::NotApplicableNeedsReason));
        skipped["reason"] = json!("não vale aqui");
        assert_eq!(checked("point", skipped), Ok(()));

        let mut by_code = base;
        by_code["status"] = json!("closed");
        by_code["result"] = json!([1]);
        by_code["closes"] = json!("MSTD-POINT-0001");
        assert_eq!(checked("point", by_code), Ok(()), "the code the page shows is accepted");
    }
}
