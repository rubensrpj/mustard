//! A reprovação do fechamento que perdeu o motivo.
//!
//! A reprovação (`validation_failure`) manda a onda ao conserto e conta no
//! limite de consertos. Ela vence, e deixa de valer, quando o motivo some:
//!
//! - o item cuja prova caiu saiu da spec (`remove`) ou ganhou versão nova
//!   (`replaces`): o fechamento prova a versão que vale hoje, e a reprovação
//!   é da versão de antes;
//! - sendo de prova que já passava antes da obra, o código de antes que ela
//!   usou não é o código de antes da obra de hoje. A reprovação desse tipo
//!   gravada sem esse commit vence.
//!
//! O fechamento e a rodada soltam a vencida no começo, pela marca nova do
//! binário (`result: released`) que aponta a reprovação (`expired`): a onda
//! deixa de voltar ao conserto, o limite de consertos não a conta, e o envio
//! de conserto aberto que só tinha ela é cancelado pela entrega do binário,
//! sem chamar agente. O fechamento, que prova tudo de novo, grava a
//! reprovação que ainda vale.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::spec_events::{Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use super::queue::{open_sends, waves_with_live_agent};
use crate::commands::flow::validation::base_commit;
use crate::commands::spec_events::write::record;

/// O motivo da reprovação de prova que já passava no código de antes da obra.
const PASSED_BEFORE: &str = "criterion-passed-before";

/// Os motivos das reprovações de prova de pedido ou de critério.
const PROOF_REASONS: [&str; 4] = ["criterion-failed", "criterion-ran-no-test", "criterion-missing-test", PASSED_BEFORE];

/// A falha do fechamento que uma onda carrega: o código do pedido ou do
/// critério quando a falha é a prova de um item, e se ela é de prova que já
/// passava antes.
pub(crate) type Ground<'a> = (Option<&'a str>, bool);

/// Os campos de vencimento da reprovação que o fechamento grava na onda, com
/// as falhas dela em `grounds` (a primeira dá o motivo da reprovação):
/// `items`, a versão vigente de cada item, vazio quando alguma falha não é
/// prova de item; e `before`, o código de antes da obra de hoje, quando o
/// motivo é prova que já passava antes.
pub(crate) fn expiry_fields(root: &Path, log: &SpecLog, grounds: &[Ground<'_>]) -> Map<String, Value> {
    let items: Vec<u64> = grounds.iter().filter_map(|(code, _)| (*code).and_then(|code| current_version(log, code))).collect();
    let items = if items.len() == grounds.len() { items } else { Vec::new() };
    let mut fields = Map::from_iter([("items".to_string(), json!(items))]);
    if grounds.first().is_some_and(|(_, passed_before)| *passed_before)
        && let Some(base) = base_commit(root, log)
    {
        fields.insert("before".into(), json!(base));
    }
    fields
}

/// O número da versão vigente do item de código `code`.
fn current_version(log: &SpecLog, code: &str) -> Option<u64> {
    log.codes().into_iter().filter(|(_, c)| c == code).find_map(|(id, _)| log.current(id)).map(|item| item.id)
}

/// Os itens que a reprovação `verdict` implica: os que ela gravou. A de antes
/// do campo cita o pedido ou o critério na primeira linha do texto, e vale a
/// versão que existia quando ela foi gravada.
fn items_of(log: &SpecLog, verdict: &SpecEvent) -> Vec<u64> {
    if verdict.fields.contains_key("items") {
        return verdict.ints("items");
    }
    if !verdict.str_field("reason").is_some_and(|reason| PROOF_REASONS.contains(&reason)) {
        return Vec::new();
    }
    let first = verdict.str_field("text").unwrap_or_default().lines().next().unwrap_or_default();
    let cited: BTreeSet<&str> = first.split(|c: char| !(c.is_alphanumeric() || c == '-')).collect();
    let mut latest: BTreeMap<String, u64> = BTreeMap::new();
    for (id, code) in log.codes() {
        let proof = log.get(id).is_some_and(|item| matches!(item.event_type.as_str(), "criterion" | "request"));
        if proof && id < verdict.id && cited.contains(code.as_str()) {
            let newest = latest.entry(code).or_insert(id);
            *newest = (*newest).max(id);
        }
    }
    latest.into_values().collect()
}

/// `true` quando a reprovação `verdict` perdeu o motivo: todos os itens dela
/// saíram da spec ou ganharam versão nova; ou ela é de prova que já passava
/// antes e usou outro código de antes que o de hoje (`today`), ou nenhum.
fn is_expired(log: &SpecLog, verdict: &SpecEvent, today: Option<&str>) -> bool {
    let items = items_of(log, verdict);
    if !items.is_empty() && items.iter().all(|id| log.current(*id).map(|item| item.id) != Some(*id)) {
        return true;
    }
    verdict.str_field("reason") == Some(PASSED_BEFORE)
        && match verdict.str_field("before") {
            None => true,
            Some(before) => today.is_some_and(|today| today != before),
        }
}

/// Solta cada onda de `pending` — a onda com o conserto pendente e a
/// reprovação que a pôs lá — cuja reprovação do fechamento venceu, e cancela
/// o envio de conserto aberto dela. A soltura é uma marca nova do binário,
/// com o resultado `released`, a mesma impressão e o mesmo motivo e o número
/// da reprovação em `expired`: a mais nova da onda, ela a tira das ondas
/// reprovadas, e o limite de consertos a pula. O envio aberto fecha com uma
/// entrega do binário, sem arquivo e sem agente, a menos que haja agente
/// vivo nele: esse entrega sozinho. Devolve as ondas soltas.
pub(crate) fn release_expired(root: &Path, spec: &str, log: &SpecLog, pending: &BTreeMap<u64, u64>, lang: Locale) -> Result<Vec<u64>, Refusal> {
    let verdicts: Vec<(u64, &SpecEvent)> = pending.iter().filter_map(|(wave, id)| log.get(*id).map(|verdict| (*wave, verdict))).collect();
    let verdicts: Vec<(u64, &SpecEvent)> = verdicts.into_iter().filter(|(_, verdict)| verdict.event_type == "validation_failure").collect();
    let today = if verdicts.iter().any(|(_, verdict)| verdict.str_field("reason") == Some(PASSED_BEFORE)) { base_commit(root, log) } else { None };
    let mut released = Vec::new();
    for (wave, verdict) in verdicts {
        if !is_expired(log, verdict, today.as_deref()) {
            continue;
        }
        let text = translate("close.verdict_expired", lang).replace("{wave}", &wave.to_string());
        let draft = json!({"wave": wave, "final": true, "result": "released", "text": text, "reason": verdict.str_field("reason"),
            "fingerprint": verdict.str_field("fingerprint"), "expired": verdict.id, "author": "binary"});
        record(root, spec, "validation_failure", draft.as_object().cloned().unwrap_or_default(), PhaseWriter::Binary)?;
        released.push(wave);
    }
    if !released.is_empty() {
        cancel_sends(root, spec, &released, lang)?;
    }
    Ok(released)
}

/// Cancela o envio de conserto aberto de cada onda de `waves`, pela entrega
/// do binário, sem arquivo. O envio com agente vivo fica: o agente entrega.
fn cancel_sends(root: &Path, spec: &str, waves: &[u64], lang: Locale) -> Result<(), Refusal> {
    let path = store::spec_file(root, spec)?;
    let Some(log) = store::read(&path)? else { return Ok(()) };
    let open = open_sends(&log);
    let alive = waves_with_live_agent(root, spec, &log);
    for wave in waves.iter().filter(|wave| open.contains_key(wave) && !alive.contains_key(wave)) {
        let text = translate("round.expired_send_cancelled", lang).replace("{wave}", &wave.to_string());
        let draft = json!({"wave": wave, "text": text, "files": [], "author": "binary"});
        record(root, spec, "delivered", draft.as_object().cloned().unwrap_or_default(), PhaseWriter::Binary)?;
    }
    Ok(())
}
