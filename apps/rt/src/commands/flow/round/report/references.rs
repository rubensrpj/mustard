//! Resolve o item que a entrega cita, pelo código da página ou pelo número,
//! até o número vigente, recusando o desconhecido e o de outro tipo.
use mustard_core::domain::spec_events::{EventRef, Refusal, SpecLog};
use serde_json::Value;

/// O número do evento que `reference` aponta, dado pelo código que a página
/// mostra ou pelo número, na versão gravada — sem conferir ainda se ele
/// segue vigente nem de que tipo é.
fn reference_id(log: &SpecLog, reference: &Value) -> Result<u64, Refusal> {
    let unknown = || Refusal::UnknownTarget {
        target: EventRef::from_value(reference)
            .unwrap_or(EventRef::Code(reference.to_string())),
    };
    let codes = log.codes();
    match reference {
        Value::Number(n) => n.as_u64().ok_or_else(unknown),
        Value::String(code) => {
            let code = code.trim();
            match code.parse::<u64>() {
                Ok(n) => Ok(n),
                Err(_) => codes.iter().filter(|(_, c)| c.as_str() == code).map(|(id, _)| *id).max().ok_or_else(unknown),
            }
        }
        _ => Err(unknown()),
    }
}

/// O número do critério `reference`, dado pelo código que a página mostra ou
/// pelo número, na versão mais nova. Um item que a spec não tem, ou que saiu
/// da leitura, é recusado como desconhecido; o item vigente de outro tipo é
/// recusado dizendo o tipo dele e o tipo que o campo pede, para quem citou
/// não sair procurando um item que existe.
pub(super) fn criterion_id(log: &SpecLog, reference: &Value) -> Result<u64, Refusal> {
    let target = || {
        EventRef::from_value(reference).unwrap_or(EventRef::Code(reference.to_string()))
    };
    let id = reference_id(log, reference)?;
    let current = log.current(id).ok_or_else(|| Refusal::UnknownTarget { target: target() })?;
    if current.event_type != "criterion" {
        return Err(Refusal::TargetOtherType { target: target(), found: current.event_type.clone(), expected: "criterion".to_string() });
    }
    Ok(current.id)
}

/// O número vigente do item combinado `reference`, dado pelo código que a
/// página mostra ou pelo número: ao contrário de [`criterion_id`], vale para
/// qualquer tipo do bloco combinado (decisão, regra, contrato...). Um item
/// que a spec não tem, ou que saiu da leitura, é recusado.
pub(in crate::commands::flow::round) fn agreed_item_id(log: &SpecLog, reference: &Value) -> Result<u64, Refusal> {
    let unknown = || Refusal::UnknownTarget {
        target: EventRef::from_value(reference)
            .unwrap_or(EventRef::Code(reference.to_string())),
    };
    let id = reference_id(log, reference)?;
    let current = log.current(id).ok_or_else(unknown)?;
    Ok(current.id)
}
