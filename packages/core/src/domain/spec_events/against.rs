//! A conferência de um evento contra o arquivo como está: o código vira o
//! número do item, o item apontado existe e é do mesmo tipo, o ponto do
//! levantamento fecha um ponto aberto e carrega a identidade dele, e a
//! remoção não tira da leitura o que não pode sair.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::domain::spec_state::original_of;
use crate::domain::survey;

use super::check::missing;
use super::read::ints;
use super::{is_empty, EventRef, Refusal, SpecEvent, SpecLog, TimeFilter};

/// Troca cada código (`MSTD-RULE-0002`) dos campos que apontam eventos pelos
/// números que ele nomeia no arquivo como está, para que a linha gravada
/// guarde só números: em `replaces` e no `closes` de um ponto, a versão mais
/// nova do item; nos alvos de `remove` e `purge`, todas as versões dele. Um
/// código que não existe na spec recusa o evento. Um evento sem código nenhum
/// sai como entrou.
pub fn resolve_codes(log: &SpecLog, event: &mut Map<String, Value>) -> Result<(), Refusal> {
    let is_code = |v: &Value| matches!(EventRef::from_value(v), Some(EventRef::Code(_)));
    let replaces_code = event.get("replaces").is_some_and(is_code);
    let closes_code = event.get("closes").is_some_and(is_code);
    let targets = event.get("targets").and_then(Value::as_array).cloned().unwrap_or_default();
    if !replaces_code && !closes_code && !targets.iter().any(is_code) {
        return Ok(());
    }
    let codes = log.codes();
    let ids_of = |code: &str| -> Result<Vec<u64>, Refusal> {
        let ids: Vec<u64> =
            log.events.iter().map(|e| e.id).filter(|id| codes.get(id).is_some_and(|c| c == code)).collect();
        if ids.is_empty() {
            Err(Refusal::UnknownTarget { target: EventRef::Code(code.to_string()) })
        } else {
            Ok(ids)
        }
    };
    for field in ["replaces", "closes"] {
        if let Some(EventRef::Code(code)) = event.get(field).and_then(EventRef::from_value) {
            let newest = ids_of(&code)?.last().copied().unwrap_or_default();
            event.insert(field.into(), Value::from(newest));
        }
    }
    if targets.iter().any(is_code) {
        let mut resolved: Vec<Value> = Vec::new();
        for target in &targets {
            let ids: Vec<Value> = match EventRef::from_value(target) {
                Some(EventRef::Code(code)) => ids_of(&code)?.into_iter().map(Value::from).collect(),
                _ => vec![target.clone()],
            };
            for id in ids {
                if !resolved.contains(&id) {
                    resolved.push(id);
                }
            }
        }
        event.insert("targets".into(), Value::Array(resolved));
    }
    Ok(())
}

/// O que uma gravação muda no resto do arquivo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Effects {
    /// Os números que um `remove` tira da leitura.
    pub removed: Vec<u64>,
    /// Os números cujo texto um `purge` tira do arquivo.
    pub purged: Vec<u64>,
}

/// Confere o evento contra o arquivo como está: o número que `replaces`
/// aponta existe e é do mesmo tipo; os alvos de `remove` e `purge` existem; o
/// filtro de `remove` acha pelo menos um evento anterior.
///
/// No ponto do levantamento: o `closes` aponta um ponto aberto, por qualquer
/// versão dele (a versão nova de um fechamento continua fechando o mesmo
/// ponto), e cada número de `result` existe. Um ponto aberto não sai com
/// `remove`, por nenhuma versão: ele só fecha, com a resposta ou o motivo. O
/// fechamento de um ponto cujo original já saiu, ou sai junto, também não sai:
/// é o único registro do ponto. O `purge` não tira item nenhum da leitura, e
/// por isso vale para qualquer item.
pub fn check_against(
    log: &SpecLog,
    event: &Map<String, Value>,
    new_id: u64,
) -> Result<Effects, Refusal> {
    let event_type = event.get("type").and_then(Value::as_str).unwrap_or_default();
    if let Some(old) = event.get("replaces").and_then(Value::as_u64) {
        let Some(previous) = log.get(old) else {
            return Err(Refusal::UnknownTarget { target: EventRef::Id(old) });
        };
        if previous.event_type != event_type {
            return Err(Refusal::ReplacesOtherType {
                id: old,
                found: previous.event_type.clone(),
                event_type: event_type.to_string(),
            });
        }
    }
    // De onde o evento veio é um evento que já está no arquivo: o número que
    // não existe, e o número do próprio evento, não dizem origem nenhuma.
    if let Some(origin) = event.get("origin").and_then(Value::as_u64)
        && (origin == new_id || log.get(origin).is_none())
    {
        return Err(Refusal::UnknownTarget { target: EventRef::Id(origin) });
    }
    let mut targets = ints(event.get("targets"));
    if let Some(unknown) = targets.iter().find(|id| log.get(**id).is_none()) {
        return Err(Refusal::UnknownTarget { target: EventRef::Id(*unknown) });
    }
    let mut effects = Effects::default();
    match event_type {
        "point" => {
            if let Some(unknown) = ints(event.get("result")).into_iter().find(|id| log.get(*id).is_none()) {
                return Err(Refusal::UnknownTarget { target: EventRef::Id(unknown) });
            }
            if let Some(target) = event.get("closes").and_then(Value::as_u64) {
                check_closes(log, event, target)?;
            } else if event.get("replaces").is_none()
                && let Some(existing) = same_open_point(log, event)
            {
                return Err(Refusal::PointAlreadyOpen {
                    code: log.codes().get(&existing.id).cloned().unwrap_or_else(|| existing.id.to_string()),
                    block: existing.str_field("block").unwrap_or_default().trim().to_string(),
                });
            }
        }
        "remove" => {
            if let Some(filter) = event.get("filter").and_then(TimeFilter::from_value) {
                let matched = log.filter_matches(&filter, new_id);
                if matched.is_empty() {
                    return Err(Refusal::FilterMatchesNothing {
                        event_type: filter.event_type,
                        from: filter.from,
                        to: filter.to,
                    });
                }
                targets.extend(matched);
            }
            targets.sort_unstable();
            targets.dedup();
            if let Some(code) = open_point_among(log, &targets) {
                return Err(Refusal::OpenPointRemoved { code });
            }
            if let Some(code) = last_record_among(log, &targets) {
                return Err(Refusal::ClosingPointLastRecord { code });
            }
            effects.removed = targets;
        }
        "purge" => {
            targets.sort_unstable();
            targets.dedup();
            effects.purged = targets;
        }
        _ => {}
    }
    Ok(effects)
}

/// O ponto aberto que já está no arquivo com o mesmo bloco e a mesma lacuna
/// do evento `event`: gravar o segundo deixaria dois pontos abertos pedindo a
/// mesma resposta, e o levantamento apresentaria a mesma pergunta duas vezes.
/// Um evento sem bloco ou sem lacuna não repete ponto nenhum.
fn same_open_point<'a>(log: &'a SpecLog, event: &Map<String, Value>) -> Option<&'a SpecEvent> {
    let text = |name: &str| event.get(name).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let (block, gap) = (text("block")?, text("gap")?);
    survey::open_points(log).into_iter().find(|point| {
        point.str_field("block").map(str::trim) == Some(block) && point.str_field("gap").map(str::trim) == Some(gap)
    })
}

/// O `closes` de um ponto aponta um ponto aberto, por qualquer versão dele. A
/// versão nova de um fechamento, que aponta o mesmo ponto que o fechamento
/// revisto, também passa. Senão, a recusa traz o código, o número e a lacuna
/// dos pontos abertos, para o próximo fechamento acertar.
fn check_closes(log: &SpecLog, event: &Map<String, Value>, target: u64) -> Result<(), Refusal> {
    let first_version = |id: u64| log.get(id).filter(|e| e.event_type == "point").map(|e| original_of(log, e));
    let open = survey::open_points(log);
    let wanted = first_version(target);
    if let Some(wanted) = wanted {
        if open.iter().any(|p| original_of(log, p) == wanted) {
            return Ok(());
        }
        let revised = event
            .get("replaces")
            .and_then(Value::as_u64)
            .and_then(|id| log.get(id))
            .and_then(|old| old.int("closes"))
            .and_then(first_version);
        if revised == Some(wanted) {
            return Ok(());
        }
    }
    let id = log.codes().get(&target).map_or_else(|| target.to_string(), |code| format!("{code} ({target})"));
    Err(Refusal::PointNotOpen { id, open: survey::describe(log, &open) })
}

/// O código do primeiro ponto aberto entre os alvos de um `remove`, por
/// qualquer versão dele; `None` quando nenhum alvo é ponto aberto.
fn open_point_among(log: &SpecLog, targets: &[u64]) -> Option<String> {
    let open: BTreeSet<u64> = survey::open_points(log).into_iter().map(|p| original_of(log, p)).collect();
    let point = targets
        .iter()
        .filter_map(|id| log.get(*id))
        .find(|e| e.event_type == "point" && open.contains(&original_of(log, e)))?;
    Some(log.codes().get(&point.id).cloned().unwrap_or_else(|| point.id.to_string()))
}

/// O código do primeiro fechamento, entre os alvos de um `remove`, que é o
/// único registro do ponto que fecha: o original dele já saiu, ou sai junto. A versão velha de um fechamento revisto pode sair,
/// porque a nova fica; `None` quando nenhum alvo é um desses fechamentos.
fn last_record_among(log: &SpecLog, targets: &[u64]) -> Option<String> {
    let leaves = |event: &SpecEvent| targets.contains(&event.id);
    let closing = survey::points(log).into_iter().find_map(|point| {
        let closing = point.closing().filter(|closing| leaves(closing))?;
        point.original().is_none_or(leaves).then_some(closing)
    })?;
    Some(log.codes().get(&closing.id).cloned().unwrap_or_else(|| closing.id.to_string()))
}

/// O ponto que fecha outro carrega a identidade dele. A versão nova
/// (`replaces`) de um fechamento recebe o mesmo `closes` da versão antiga,
/// qualquer que seja o que veio no pedido, e por isso nunca tira o ponto da
/// leitura. Depois, a lacuna (`gap`) e a origem (`from`) do ponto fechado,
/// lidas pelo par, entram no fechamento, qualquer que seja a lacuna que veio
/// no pedido: a lacuna segue coberta pelo fechamento depois que o original
/// sai. Outro evento sai como entrou.
///
/// Com o `closes` no lugar, o ponto é conferido de novo: a versão que tenta
/// reabrir o ponto é recusada como todo ponto aberto que fecha outro, e o
/// ponto que não está aberto e segue sem `closes` é recusado pela falta dele.
pub fn carry_closed_identity(log: &SpecLog, event: &mut Map<String, Value>) -> Result<(), Refusal> {
    if event.get("type").and_then(Value::as_str) != Some("point") {
        return Ok(());
    }
    let inherited = event
        .get("replaces")
        .and_then(Value::as_u64)
        .and_then(|id| log.get(id))
        .filter(|old| old.event_type == "point")
        .and_then(|old| old.int("closes"));
    if let Some(closes) = inherited {
        event.insert("closes".into(), Value::from(closes));
    }
    let open = event.get("status").and_then(Value::as_str) == Some("open");
    match (open, event.get("closes").is_some_and(|v| !is_empty(v))) {
        (true, true) => return Err(Refusal::ClosingPointOpen),
        (false, false) => return Err(missing("point", "closes")),
        _ => {}
    }
    let Some(target) = event.get("closes").and_then(Value::as_u64).and_then(|id| log.get(id)) else {
        return Ok(());
    };
    if target.event_type != "point" {
        return Ok(());
    }
    let first = original_of(log, target);
    let Some(point) = survey::points(log).into_iter().find(|point| point.first() == first) else {
        return Ok(());
    };
    for field in ["gap", "from"] {
        if let Some(value) = point.shown().fields.get(field) {
            event.insert(field.to_string(), value.clone());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::spec_events::tests::{checked, line, obj};
    use crate::domain::spec_events::{parse_log, Kind};
    use crate::platform::i18n::Locale;

    /// Um código aponta o item: em `replaces`, a versão mais nova; nos alvos,
    /// todas as versões. Um código que não existe é recusado citando o código,
    /// nos dois idiomas, e um texto fora do formato nem passa da conferência.
    #[test]
    fn a_code_points_at_its_item_and_an_unknown_code_is_refused() {
        let log = parse_log(
            &[
                line(1, "rule", ",\"code\":\"MSTD-RULE-0001\""),
                line(2, "rule", ",\"code\":\"MSTD-RULE-0002\""),
                line(3, "rule", ",\"code\":\"MSTD-RULE-0002\",\"replaces\":2"),
            ]
            .concat(),
        );
        let mut removal = obj(json!({"type": "remove", "targets": ["MSTD-RULE-0002", 1, 3], "reason": "r"}));
        resolve_codes(&log, &mut removal).unwrap();
        assert_eq!(removal["targets"], json!([2, 3, 1]));
        let mut revision = obj(json!({"type": "rule", "replaces": "MSTD-RULE-0002"}));
        resolve_codes(&log, &mut revision).unwrap();
        assert_eq!(revision["replaces"], json!(3));

        let mut unknown = obj(json!({"type": "purge", "targets": ["MSTD-RULE-0009"], "reason": "secret"}));
        let refusal = resolve_codes(&log, &mut unknown).unwrap_err();
        assert_eq!(refusal, Refusal::UnknownTarget { target: EventRef::Code("MSTD-RULE-0009".into()) });
        assert_eq!(refusal.reason(), "unknown-target");
        assert!(refusal.message(Locale::PtBr).contains("O item MSTD-RULE-0009 não existe nesta spec"));
        assert!(refusal.message(Locale::EnUs).contains("Item MSTD-RULE-0009 does not exist in this spec"));

        assert!(checked("remove", json!({"targets": ["MSTD-RULE-0002"], "reason": "r"})).is_ok());
        assert!(matches!(
            checked("remove", json!({"targets": ["R2"], "reason": "r"})).unwrap_err(),
            Refusal::InvalidValue { ref field, expected: Kind::Refs, .. } if field == "targets"
        ));
    }
}
