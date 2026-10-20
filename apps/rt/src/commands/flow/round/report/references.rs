//! Resolve o item que a entrega cita, pelo código da página ou pelo número,
//! até o número vigente, recusando o desconhecido e o de outro tipo, e o
//! critério onde vale a prova que cita outro item.
use std::collections::BTreeSet;

use mustard_core::domain::spec_events::{EventRef, Refusal, SpecEvent, SpecLog};
use mustard_core::platform::i18n::{Locale, translate};
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

/// O evento vigente que `reference` aponta, dado pelo código que a página
/// mostra ou pelo número. Um item que a spec não tem, ou que saiu da
/// leitura, é recusado como desconhecido.
fn current_item<'a>(log: &'a SpecLog, reference: &Value) -> Result<&'a SpecEvent, Refusal> {
    let id = reference_id(log, reference)?;
    log.current(id).ok_or_else(|| Refusal::UnknownTarget {
        target: EventRef::from_value(reference).unwrap_or(EventRef::Code(reference.to_string())),
    })
}

/// O número do critério `reference`, dado pelo código que a página mostra ou
/// pelo número, na versão mais nova. Um item que a spec não tem, ou que saiu
/// da leitura, é recusado como desconhecido; o item vigente de outro tipo é
/// recusado dizendo o tipo dele e o tipo que o campo pede, para quem citou
/// não sair procurando um item que existe.
pub(super) fn criterion_id(log: &SpecLog, reference: &Value) -> Result<u64, Refusal> {
    let current = current_item(log, reference)?;
    if current.event_type != "criterion" {
        let target = EventRef::from_value(reference).unwrap_or(EventRef::Code(reference.to_string()));
        return Err(Refusal::TargetOtherType { target, found: current.event_type.clone(), expected: "criterion".to_string() });
    }
    Ok(current.id)
}

/// Onde vale a prova que a entrega dá a um item: a mesma resposta na
/// gravação da entrega e na rodada que a assume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProofTarget {
    /// O critério citado.
    Criterion(u64),
    /// A tarefa citada cobre um critério só, e a prova vale para ele.
    Rerouted { task: u64, criterion: u64 },
    /// O item citado não é critério e não leva a um critério só: a prova
    /// fica como dele, sem versão nova de critério nenhum.
    Kept(u64),
}

impl ProofTarget {
    /// O critério que ganha a prova, quando há um.
    pub(super) fn criterion(self) -> Option<u64> {
        match self {
            Self::Criterion(id) | Self::Rerouted { criterion: id, .. } => Some(id),
            Self::Kept(_) => None,
        }
    }

    /// O número do item cujo código a regra da prova nomeia: o critério que a
    /// ganha, ou o item citado quando nenhum critério a ganha.
    pub(super) fn named(self) -> u64 {
        match self {
            Self::Criterion(id) | Self::Rerouted { criterion: id, .. } | Self::Kept(id) => id,
        }
    }

    /// O motivo e o texto do aviso da prova que não citou um critério, no
    /// idioma `lang`; `None` quando ela citou.
    pub(super) fn warning(self, log: &SpecLog, lang: Locale) -> Option<(&'static str, String)> {
        let codes = log.codes();
        let code = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
        match self {
            Self::Criterion(_) => None,
            Self::Rerouted { task, criterion } => Some((
                "proof-rerouted",
                translate("round.proof_rerouted", lang).replace("{item}", &code(task)).replace("{criterion}", &code(criterion)),
            )),
            Self::Kept(id) => {
                let kind = log.current(id).map(|item| item.event_type.clone()).unwrap_or_default();
                Some(("proof-kept", translate("round.proof_kept", lang).replace("{item}", &code(id)).replace("{type}", &kind)))
            }
        }
    }
}

/// Onde vale a prova que cita `reference`. O critério vale como veio; a
/// tarefa que cobre um critério só passa a prova a ele; qualquer outro item
/// que a spec tem — a tarefa que cobre nenhum ou vários critérios, o pedido,
/// a decisão — fica com a prova, sem critério. Só o item que a spec não tem,
/// ou que saiu da leitura, é recusado.
pub(super) fn proof_target(log: &SpecLog, reference: &Value) -> Result<ProofTarget, Refusal> {
    let item = current_item(log, reference)?;
    match item.event_type.as_str() {
        "criterion" => Ok(ProofTarget::Criterion(item.id)),
        "task" => {
            let covered: BTreeSet<u64> =
                item.ints("covers").into_iter().filter_map(|id| log.current(id)).filter(|c| c.event_type == "criterion").map(|c| c.id).collect();
            match covered.into_iter().collect::<Vec<_>>()[..] {
                [criterion] => Ok(ProofTarget::Rerouted { task: item.id, criterion }),
                _ => Ok(ProofTarget::Kept(item.id)),
            }
        }
        _ => Ok(ProofTarget::Kept(item.id)),
    }
}

/// O número vigente do item combinado `reference`, dado pelo código que a
/// página mostra ou pelo número: ao contrário de [`criterion_id`], vale para
/// qualquer tipo do bloco combinado (decisão, regra, contrato...). Um item
/// que a spec não tem, ou que saiu da leitura, é recusado.
pub(in crate::commands::flow::round) fn agreed_item_id(log: &SpecLog, reference: &Value) -> Result<u64, Refusal> {
    Ok(current_item(log, reference)?.id)
}
