//! A conferência de um evento contra o arquivo como está: o código vira o
//! número do item, o item apontado existe e é do mesmo tipo, o ponto do
//! levantamento fecha um ponto aberto e carrega a identidade dele, e a
//! remoção não tira da leitura o que não pode sair.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::domain::spec_state::original_of;
use crate::domain::survey;

use super::check::missing;
use super::read::ints;
use super::{is_empty, Block, BlockQuery, EventRef, Hidden, Refusal, SpecEvent, SpecLog, TimeFilter};

/// Troca cada código (`MSTD-RULE-NNNN`) dos campos que apontam eventos pelos
/// números que ele nomeia no arquivo como está, para que a linha gravada
/// guarde só números: em `replaces` (um só ou a lista) e no `closes` de um
/// ponto, a versão mais nova do item; nos alvos de `remove` e `purge`, todas
/// as versões dele. Um código que não existe na spec recusa o evento.
///
/// Na remoção, a tarefa sai sempre inteira: o número de qualquer versão dela,
/// e a versão dela que o filtro pega, viram todas as versões da tarefa nos
/// alvos. A remoção gravada já leva o efeito, e a leitura segue a mesma:
/// tirar só a versão que a rodada gravou com a onda devolveria a versão sem
/// onda ao backlog, e a rodada a soltaria de novo. Nos outros tipos, o número
/// tira só aquela versão. O evento sem código e sem tarefa a expandir sai
/// como entrou.
pub fn resolve_codes(log: &SpecLog, event: &mut Map<String, Value>) -> Result<(), Refusal> {
    let is_code = |v: &Value| matches!(EventRef::from_value(v), Some(EventRef::Code(_)));
    let replaces_list = event.get("replaces").and_then(Value::as_array).cloned();
    let replaces_code = event.get("replaces").is_some_and(is_code)
        || replaces_list.as_ref().is_some_and(|list| list.iter().any(is_code));
    let closes_code = event.get("closes").is_some_and(is_code);
    let targets = event.get("targets").and_then(Value::as_array).cloned().unwrap_or_default();
    let removal = event.get("type").and_then(Value::as_str) == Some("remove");
    if !replaces_code && !closes_code && !targets.iter().any(is_code) && !removal {
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
    // A lista de `replaces` troca cada código pela versão mais nova dele, na
    // mesma posição; o número que já veio fica como está.
    if let Some(list) = replaces_list.filter(|list| list.iter().any(is_code)) {
        let mut resolved = Vec::with_capacity(list.len());
        for item in &list {
            resolved.push(match EventRef::from_value(item) {
                Some(EventRef::Code(code)) => Value::from(ids_of(&code)?.last().copied().unwrap_or_default()),
                _ => item.clone(),
            });
        }
        event.insert("replaces".into(), Value::Array(resolved));
    }
    // Todas as versões da tarefa de que `id` é uma versão; `None` quando o
    // número não é de tarefa, e ele segue sozinho.
    let task_versions = |id: u64| -> Option<Vec<u64>> {
        log.get(id).filter(|e| e.event_type == "task")?;
        codes.get(&id).and_then(|code| ids_of(code).ok())
    };
    let mut changed = false;
    let mut resolved: Vec<Value> = Vec::new();
    for target in &targets {
        let ids: Vec<Value> = match EventRef::from_value(target) {
            Some(EventRef::Code(code)) => {
                changed = true;
                ids_of(&code)?.into_iter().map(Value::from).collect()
            }
            Some(EventRef::Id(id)) if removal => match task_versions(id) {
                Some(versions) => {
                    changed |= versions != [id];
                    versions.into_iter().map(Value::from).collect()
                }
                None => vec![target.clone()],
            },
            _ => vec![target.clone()],
        };
        for id in ids {
            if !resolved.contains(&id) {
                resolved.push(id);
            }
        }
    }
    // A versão de tarefa que o filtro pega leva junto as versões dela que ele
    // não pega, como a sem onda, gravada antes do intervalo.
    if removal && let Some(filter) = event.get("filter").and_then(TimeFilter::from_value) {
        let matched: BTreeSet<u64> = log.filter_matches(&filter, log.max_id().saturating_add(1)).into_iter().collect();
        for version in matched.iter().filter_map(|id| task_versions(*id)).flatten() {
            if !matched.contains(&version) && !resolved.contains(&Value::from(version)) {
                changed = true;
                resolved.push(Value::from(version));
            }
        }
    }
    if changed {
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

/// Confere o evento contra o arquivo como está: cada número que `replaces`
/// aponta, um só ou a lista, existe, é do mesmo tipo e é a versão vigente do
/// item, e na tarefa não é versão que uma remoção tirou; os alvos de `remove`
/// e `purge` existem; o filtro de `remove` acha pelo menos um evento anterior.
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
    // `None` quando o evento não é a emenda de nada; `Some(true)` ou
    // `Some(false)` quando é, conforme a versão substituída já declarava a
    // forma do critério ou não.
    let mut replaces_form = None;
    let replaced = event.get("replaces").and_then(Value::as_u64).map_or_else(|| ints(event.get("replaces")), |old| vec![old]);
    for old in replaced {
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
        // A versão nova só substitui a vigente: substituir uma versão que já
        // tem sucessora dividiria o item em duas pontas, e cada leitura
        // seguiria uma. A recusa diz a vigente, para quem grava reler e
        // gravar por cima dela. Sem versão vigente, com o item fora da
        // leitura, nada se divide.
        if let Some(current) = log.current(old).filter(|current| current.id != old) {
            return Err(Refusal::ReplacesSuperseded {
                id: old,
                current: log.codes().get(&current.id).map_or_else(
                    || current.id.to_string(),
                    |code| format!("{code} ({})", current.id),
                ),
            });
        }
        // A tarefa que uma remoção tirou não volta por uma versão nova: a
        // versão nova sobre uma versão removida traria de volta o trabalho
        // que o usuário tirou. Refazer é gravar uma tarefa nova.
        if event_type == "task"
            && let Some(Hidden::Removed { by }) = log.hidden().get(&old)
        {
            return Err(Refusal::ReplacesRemoved { id: old, by: *by });
        }
        // Basta uma versão substituída sem a forma para a emenda herdar a
        // ausência dela.
        if event_type == "criterion" {
            let has_form = previous.str_field("form").is_some_and(|form| !form.trim().is_empty());
            replaces_form = Some(replaces_form.unwrap_or(true) && has_form);
        }
    }
    // A forma é obrigatória para o critério que nasce agora; a emenda de um
    // critério escrito antes de a forma virar campo obrigatório herda a
    // ausência dela, sem travar. A emenda de um critério que já declarava
    // forma continua exigindo o campo.
    if event_type == "criterion" && event.get("form").is_none_or(is_empty) && replaces_form != Some(false) {
        return Err(Refusal::CriterionFormMissing);
    }
    if event_type == "task" {
        check_wave_dependencies(log, event)?;
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
                check_answer_has_facts(log, event, target)?;
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

/// A tarefa gravada, nova ou versão, com a onda N que ainda não saiu (sem
/// envio gravado) só depende de tarefa que a rodada entrega antes dela: a
/// versão vigente de cada dependência está numa onda entregue, na própria
/// onda N, ou numa onda que a N espera, direto ou por outra onda. Senão a
/// rodada soltaria a onda N antes da dependência. A onda que já saiu fica de
/// fora: o pedido dela já foi feito. A dependência que não aponta tarefa
/// vigente nenhuma não entra aqui: quem a recusa é a conferência da tarefa
/// nova, e a rodada a ignora.
fn check_wave_dependencies(log: &SpecLog, event: &Map<String, Value>) -> Result<(), Refusal> {
    let Some(wave) = event.get("wave").and_then(Value::as_u64) else {
        return Ok(());
    };
    let depends_on = event.get("depends_on").and_then(Value::as_array).cloned().unwrap_or_default();
    if depends_on.is_empty() {
        return Ok(());
    }
    let waves = log.block(BlockQuery::Block(Block::Waves));
    if waves.iter().any(|e| e.event_type == "send" && e.wave() == Some(wave)) {
        return Ok(());
    }
    // A própria onda e cada onda que ela espera pelo evento de onda, direto
    // ou por outra onda.
    let mut reached = BTreeSet::from([wave]);
    let mut pending = vec![wave];
    while let Some(n) = pending.pop() {
        let waits = waves.iter().filter(|e| e.event_type == "wave" && e.wave() == Some(n)).flat_map(|e| e.ints("depends_on"));
        for m in waits.collect::<Vec<_>>() {
            if reached.insert(m) {
                pending.push(m);
            }
        }
    }
    let delivered = log.delivered_waves();
    let codes = log.codes();
    let mut missing = Vec::new();
    for value in &depends_on {
        let Some(task) = current_task(log, &codes, value) else { continue };
        if !task.wave().is_some_and(|m| reached.contains(&m) || delivered.contains(&m)) {
            let label = codes.get(&task.id).map_or_else(|| task.id.to_string(), |code| format!("{code} ({})", task.id));
            if !missing.contains(&label) {
                missing.push(label);
            }
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(Refusal::DependsOutsideWave { wave, missing })
    }
}

/// A versão vigente da tarefa que `value` aponta, pelo número de qualquer
/// versão ou pelo código; `None` quando não aponta tarefa vigente nenhuma.
fn current_task<'a>(log: &'a SpecLog, codes: &BTreeMap<u64, String>, value: &Value) -> Option<&'a SpecEvent> {
    let id = match EventRef::from_value(value)? {
        EventRef::Id(id) => id,
        EventRef::Code(code) => log.events.iter().filter(|e| codes.get(&e.id) == Some(&code)).map(|e| e.id).next_back()?,
    };
    log.current(id).filter(|task| task.event_type == "task")
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

/// O ponto que fecha com resposta (`result`) precisa de fato: no fechamento,
/// na versão vigente do ponto ou no fechamento que ele revê. O ponto que o
/// `grill` gravou nasce sem fato, e responder a ele sem conferir nada no
/// código nem na conversa é o que esta recusa barra. O "não se aplica" passa
/// sem fato, com o motivo.
fn check_answer_has_facts(log: &SpecLog, event: &Map<String, Value>, target: u64) -> Result<(), Refusal> {
    let answered = event.get("result").is_some_and(|v| !is_empty(v));
    let not_applicable = event.get("status").and_then(Value::as_str) == Some("not_applicable");
    let with_facts = |fields: &Map<String, Value>| fields.get("facts").is_some_and(|v| !is_empty(v));
    if !answered || not_applicable || with_facts(event) {
        return Ok(());
    }
    let Some(first) = log.get(target).filter(|e| e.event_type == "point").map(|e| original_of(log, e)) else {
        return Ok(());
    };
    let Some(point) = survey::points(log).into_iter().find(|point| point.first() == first) else {
        return Ok(());
    };
    if with_facts(&point.shown().fields) || point.closing().is_some_and(|closing| with_facts(&closing.fields)) {
        return Ok(());
    }
    let code = log.codes().get(&target).cloned().unwrap_or_else(|| target.to_string());
    Err(Refusal::PointWithoutFacts { code })
}

/// A versão nova de um ponto aberto que traz só os fatos (`replaces` e
/// `facts`, sem `status` nem `closes`) recebe da versão antiga o bloco, a
/// lacuna, a origem do ponto, a situação, a mensagem de origem e os
/// lembretes, e os fatos dela somados aos novos, sem repetir. É como o
/// assistente acrescenta os fatos ao ponto que o `grill` gravou sem copiar o
/// ponto inteiro. Roda antes da conferência do evento sozinho, que pede esses
/// campos. A versão antiga já gravada não muda, então lê-la antes da trava
/// não perde nada. Outro evento sai como entrou.
pub fn carry_open_point(log: &SpecLog, event: &mut Map<String, Value>) {
    if event.contains_key("status") || event.contains_key("closes") {
        return;
    }
    let codes = log.codes();
    let old = match event.get("replaces").and_then(EventRef::from_value) {
        Some(EventRef::Id(id)) => log.get(id),
        Some(EventRef::Code(code)) => {
            log.events.iter().rev().find(|e| codes.get(&e.id).is_some_and(|c| *c == code))
        }
        None => None,
    };
    let Some(old) = old.filter(|old| old.event_type == "point" && old.str_field("status") == Some("open")) else {
        return;
    };
    for field in ["block", "gap", "from", "status", "origin", "reminders"] {
        if let Some(value) = old.fields.get(field).filter(|_| !event.contains_key(field)) {
            event.insert(field.to_string(), value.clone());
        }
    }
    let mut facts: Vec<Value> = old.fields.get("facts").and_then(Value::as_array).cloned().unwrap_or_default();
    for fact in event.get("facts").and_then(Value::as_array).cloned().unwrap_or_default() {
        if !facts.contains(&fact) {
            facts.push(fact);
        }
    }
    if !facts.is_empty() {
        event.insert("facts".into(), Value::Array(facts));
    }
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

    /// Na remoção, o número de qualquer versão de uma tarefa vira todas as
    /// versões dela, na posição do número; o de uma versão de nota segue
    /// sozinho. A versão de tarefa que o filtro pega leva junto as versões
    /// que ele não pega. O expurgo não expande nada.
    #[test]
    fn removing_any_version_of_a_task_takes_out_every_version() {
        let at = |id: u64, time: &str, event_type: &str, extra: &str| {
            format!("{{\"v\":1,\"id\":{id},\"at\":\"2026-09-12T{time}:00-03:00\",\"type\":\"{event_type}\"{extra}}}\n")
        };
        let log = parse_log(
            &[
                at(1, "10:00", "task", ",\"code\":\"MSTD-TASK-0001\""),
                at(2, "10:00", "note", ",\"code\":\"MSTD-NOTE-0001\""),
                at(3, "11:00", "task", ",\"code\":\"MSTD-TASK-0001\",\"replaces\":1,\"wave\":1"),
                at(4, "11:00", "note", ",\"code\":\"MSTD-NOTE-0001\",\"replaces\":2"),
                at(5, "12:00", "task", ",\"code\":\"MSTD-TASK-0002\""),
            ]
            .concat(),
        );
        let resolved = |draft: Value| {
            let mut event = obj(draft);
            resolve_codes(&log, &mut event).unwrap();
            event
        };

        let by_the_wave_version = resolved(json!({"type": "remove", "targets": [3], "reason": "r"}));
        assert_eq!(by_the_wave_version["targets"], json!([1, 3]), "a versão com onda leva a sem onda junto");
        let mixed = resolved(json!({"type": "remove", "targets": [4, 1], "reason": "r"}));
        assert_eq!(mixed["targets"], json!([4, 1, 3]), "a nota fica só na versão apontada; a tarefa vai inteira");
        let note_only = resolved(json!({"type": "remove", "targets": [4], "reason": "r"}));
        assert_eq!(note_only["targets"], json!([4]), "a versão de nota segue sozinha");
        let single = resolved(json!({"type": "remove", "targets": [5], "reason": "r"}));
        assert_eq!(single["targets"], json!([5]), "a tarefa de versão única segue como está");

        let by_filter = resolved(json!({"type": "remove", "reason": "r",
            "filter": {"type": "task", "from": "2026-09-12T11:00", "to": "2026-09-12T11:59"}}));
        assert_eq!(by_filter["targets"], json!([1]), "o filtro pega a versão com onda, e a sem onda entra nos alvos");
        let note_filter = resolved(json!({"type": "remove", "reason": "r",
            "filter": {"type": "note", "from": "2026-09-12T11:00", "to": "2026-09-12T11:59"}}));
        assert_eq!(note_filter.get("targets"), None, "o filtro de nota sai como entrou");

        let purge = resolved(json!({"type": "purge", "targets": [3], "reason": "secret"}));
        assert_eq!(purge["targets"], json!([3]), "o expurgo não tira item da leitura e não expande");
    }

    /// A versão nova de uma tarefa sobre uma versão que a remoção tirou é
    /// recusada, dizendo o evento e a remoção, nos dois idiomas. A versão
    /// nova de uma nota removida segue aceita, como antes.
    #[test]
    fn a_new_version_of_a_removed_task_is_refused() {
        let log = parse_log(
            &[
                line(1, "task", ",\"code\":\"MSTD-TASK-0001\""),
                line(2, "task", ",\"code\":\"MSTD-TASK-0001\",\"replaces\":1,\"wave\":1"),
                line(3, "remove", ",\"targets\":[1,2],\"reason\":\"r\",\"gives_back\":true"),
                line(4, "note", ",\"code\":\"MSTD-NOTE-0001\""),
                line(5, "remove", ",\"targets\":[4],\"reason\":\"r\",\"gives_back\":true"),
            ]
            .concat(),
        );
        let over = |kind: &str, replaces: u64| check_against(&log, &obj(json!({"type": kind, "replaces": replaces})), 6);

        for version in [1, 2] {
            let refusal = over("task", version).unwrap_err();
            assert_eq!(refusal, Refusal::ReplacesRemoved { id: version, by: 3 });
            assert_eq!(refusal.reason(), "replaces-removed");
        }
        let refusal = over("task", 2).unwrap_err();
        let pt = refusal.message(Locale::PtBr);
        assert!(pt.contains("O evento 2 é de uma tarefa que a remoção 3 tirou."), "{pt}");
        assert!(pt.contains("grave uma tarefa nova, sem replaces. Nada foi gravado."), "{pt}");
        let en = refusal.message(Locale::EnUs);
        assert!(en.contains("Event 2 belongs to a task that removal 3 took out."), "{en}");
        assert!(en.contains("write a new task, without replaces. Nothing was written."), "{en}");

        assert!(over("note", 4).is_ok(), "a nota removida segue a regra de antes");
    }

    /// A tarefa numa onda que ainda não saiu só depende de tarefa que sai
    /// antes dela. Onda 1 entregue com a tarefa 1; onda 2 sem envio com a
    /// tarefa 2; onda 3 sem envio, que espera a 4, que espera a 5, com as
    /// tarefas 4 e 7 na 3 e a 5 na 5; a tarefa 3 no backlog; onda 6 já
    /// enviada.
    #[test]
    fn a_task_in_a_wave_not_yet_sent_depends_only_on_what_goes_out_before_it() {
        let log = parse_log(
            &[
                line(1, "wave", ",\"n\":1"),
                line(2, "task", ",\"code\":\"MSTD-TASK-0001\",\"wave\":1"),
                line(3, "send", ",\"wave\":1"),
                line(4, "delivered", ",\"wave\":1"),
                line(5, "wave", ",\"n\":2"),
                line(6, "task", ",\"code\":\"MSTD-TASK-0002\",\"wave\":2"),
                line(7, "task", ",\"code\":\"MSTD-TASK-0003\""),
                line(8, "wave", ",\"n\":3,\"depends_on\":[4]"),
                line(9, "wave", ",\"n\":4,\"depends_on\":[5]"),
                line(10, "wave", ",\"n\":5"),
                line(11, "task", ",\"code\":\"MSTD-TASK-0004\",\"wave\":3"),
                line(12, "task", ",\"code\":\"MSTD-TASK-0005\",\"wave\":5"),
                line(13, "wave", ",\"n\":6"),
                line(14, "task", ",\"code\":\"MSTD-TASK-0006\",\"wave\":6"),
                line(15, "send", ",\"wave\":6"),
                line(16, "task", ",\"code\":\"MSTD-TASK-0007\",\"wave\":3"),
                line(17, "task", ",\"code\":\"MSTD-TASK-0003\",\"replaces\":7"),
            ]
            .concat(),
        );
        let version = |wave: Option<u64>, depends_on: Value| {
            let mut draft = obj(json!({"type": "task", "replaces": 11, "depends_on": depends_on}));
            if let Some(n) = wave {
                draft.insert("wave".into(), json!(n));
            }
            check_against(&log, &draft, 18)
        };
        let backlog = Refusal::DependsOutsideWave { wave: 3, missing: vec!["MSTD-TASK-0003 (17)".into()] };

        assert_eq!(version(Some(3), json!(["MSTD-TASK-0003"])), Err(backlog.clone()), "o backlog pelo código");
        assert_eq!(version(Some(3), json!([7])), Err(backlog.clone()), "o backlog por uma versão velha");
        let new_task = obj(json!({"type": "task", "wave": 3, "depends_on": [17]}));
        assert_eq!(check_against(&log, &new_task, 18), Err(backlog), "a tarefa nova segue a mesma regra");
        assert_eq!(
            version(Some(3), json!([2, 17, "MSTD-TASK-0002", 6])),
            Err(Refusal::DependsOutsideWave {
                wave: 3,
                missing: vec!["MSTD-TASK-0003 (17)".into(), "MSTD-TASK-0002 (6)".into()],
            }),
            "a onda sem envio que a onda não espera também falta, e cada tarefa sai uma vez"
        );

        assert_eq!(version(Some(3), json!([2])).map(|_| ()), Ok(()), "a onda entregue");
        assert_eq!(version(Some(3), json!([16])).map(|_| ()), Ok(()), "a mesma onda");
        assert_eq!(version(Some(3), json!(["MSTD-TASK-0005"])).map(|_| ()), Ok(()), "a onda que ela espera, por outra");
        assert_eq!(version(None, json!([17, 6])).map(|_| ()), Ok(()), "sem onda, o backlog empacota na ordem");
        assert_eq!(version(Some(6), json!([17, 6])).map(|_| ()), Ok(()), "a onda que já saiu fica de fora");

        let refusal = version(Some(3), json!([17])).unwrap_err();
        assert_eq!(refusal.reason(), "depends-outside-wave");
        let pt = refusal.message(Locale::PtBr);
        assert!(pt.contains("A tarefa vai para a onda 3, que ainda não saiu, e depende de MSTD-TASK-0003 (17)."), "{pt}");
        assert!(pt.contains("Grave a tarefa sem wave: o backlog a põe numa onda depois das dependências."), "{pt}");
        let en = refusal.message(Locale::EnUs);
        assert!(en.contains("The task goes to wave 3, which has not gone out yet, and depends on MSTD-TASK-0003 (17)."), "{en}");
        assert!(en.contains("Write the task without wave: the backlog puts it in a wave after its dependencies."), "{en}");
    }

    /// Com a versão 2 no lugar da 1, a versão nova sobre a 1 é recusada, e a
    /// recusa diz a 2 pelo código e pelo número, nos dois idiomas; sobre a 2
    /// passa. Na lista, basta um alvo já substituído para recusar. O tipo sem
    /// código diz a vigente só pelo número, e o item que saiu inteiro da
    /// leitura, sem versão vigente, não recusa.
    #[test]
    fn a_new_version_replaces_only_the_current_version_of_the_item() {
        let log = parse_log(
            &[
                line(1, "rule", ",\"code\":\"MSTD-RULE-0001\""),
                line(2, "rule", ",\"code\":\"MSTD-RULE-0001\",\"replaces\":1"),
                line(3, "rule", ",\"code\":\"MSTD-RULE-0002\""),
                line(4, "future_kind", ""),
                line(5, "future_kind", ",\"replaces\":4"),
                line(6, "rule", ",\"code\":\"MSTD-RULE-0003\""),
                line(7, "rule", ",\"code\":\"MSTD-RULE-0003\",\"replaces\":6"),
                line(8, "remove", ",\"targets\":[7],\"reason\":\"r\""),
            ]
            .concat(),
        );
        let over = |kind: &str, replaces: Value| {
            check_against(&log, &obj(json!({"type": kind, "replaces": replaces})), 9)
        };
        let superseded = Refusal::ReplacesSuperseded { id: 1, current: "MSTD-RULE-0001 (2)".into() };

        let refusal = over("rule", json!(1)).unwrap_err();
        assert_eq!(refusal, superseded);
        assert_eq!(refusal.reason(), "replaces-superseded");
        let pt = refusal.message(Locale::PtBr);
        assert!(pt.contains("O evento 1 já foi substituído, e a versão vigente do item é MSTD-RULE-0001 (2)."), "{pt}");
        let en = refusal.message(Locale::EnUs);
        assert!(en.contains("Event 1 was already replaced, and the item's current version is MSTD-RULE-0001 (2)."), "{en}");
        assert!(over("rule", json!(2)).is_ok(), "sobre a vigente passa");

        assert_eq!(over("rule", json!([3, 1])).unwrap_err(), superseded, "um alvo substituído recusa a lista");
        assert!(over("rule", json!([3, 2])).is_ok(), "a lista de vigentes passa");

        assert_eq!(
            over("future_kind", json!(4)).unwrap_err(),
            Refusal::ReplacesSuperseded { id: 4, current: "5".into() },
            "sem código, a vigente vai pelo número"
        );
        assert!(over("rule", json!(6)).is_ok(), "o item fora da leitura não tem vigente");
    }
}
