//! A reprovação de quem conduz a obra: a linha `REJECTED` do relatório, com a
//! onda e o motivo. A rodada grava a reprovação na spec, presa à volta que ela
//! reprova, segura essa volta fora do commit pelo mesmo caminho da volta que
//! ela mesma recusa ([`HeldReturn`]) e leva o motivo ao agente novo da onda,
//! pelo arquivo do trecho de conserto que o gancho do despacho lê. Enquanto a
//! volta reprovada for a última da onda, cada rodada segue segurando-a; a
//! volta nova desfaz a reprovação, e a rodada a assume como qualquer outra.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::domain::wave_prompt::wave_title;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

use super::answer::{without_final_period, RoundRefusal};
use super::fixes::fix_file;
use super::queue::{send_revision, waves_awaiting_new_agent};
use super::report::{line_object, tagged};
use super::stops::HeldReturn;
use super::usage::{measure_usage, usage_missing, Caller, Usage};
use crate::commands::spec_events::write::record;

/// A linha com que quem conduz a obra reprova a volta de uma onda, com o
/// motivo: `<REJECTED>{"wave":1,"reason":"…"}</REJECTED>`. Ela também marca
/// que o agente terminou, como a `USAGE`: o consumo dele é medido junto, sem a
/// outra linha.
const REJECTED_LINE: &str = "REJECTED";

/// Cada linha `REJECTED` do relatório `raw`, com a onda e o motivo, na ordem.
/// A linha sem a onda, sem o motivo ou com o motivo em branco é recusada pelo
/// campo que falta.
pub(super) fn rejected_lines(raw: &str) -> Result<Vec<(u64, String)>, RoundRefusal> {
    let mut rejected = Vec::new();
    for body in tagged(raw, REJECTED_LINE) {
        let (wave, fields) = line_object(body, REJECTED_LINE)?;
        let wave = wave.ok_or(RoundRefusal::LineField { line: REJECTED_LINE, field: "wave" })?;
        let reason = fields.get("reason").and_then(Value::as_str).map(str::trim).filter(|said| !said.is_empty());
        let reason = reason.ok_or(RoundRefusal::LineField { line: REJECTED_LINE, field: "reason" })?;
        rejected.push((wave, reason.to_string()));
    }
    Ok(rejected)
}

/// A reprovação que quem conduz a obra gravou para a onda `wave`, lida do
/// envio mais novo dela: o número da volta reprovada e o motivo. A versão do
/// envio que o despacho de um agente novo grava depois a leva junto.
pub(super) fn rejection_of(log: &SpecLog, wave: u64) -> Option<(u64, String)> {
    let sent = log.get(*log.last_by_wave("send").get(&wave)?)?;
    let rejected = sent.fields.get("rejected")?;
    Some((rejected.get("delivered")?.as_u64()?, rejected.get("reason")?.as_str()?.to_string()))
}

/// Se o agente da onda `wave` cuja conversa começou em `started` é um que a
/// reprovação de quem conduz a obra tirou dela: a conversa dele começou antes
/// da volta reprovada, então foi ele, ou um agente ainda mais antigo, quem a
/// fez. O agente novo nasce depois da reprovação e segue dono da onda, também
/// quando a rodada recusar a volta dele. Nada sem reprovação gravada para a
/// onda, nem quando a hora da volta reprovada não se lê.
pub(crate) fn replaced_by_rejection(log: &SpecLog, wave: u64, started: DateTime<Utc>) -> bool {
    let Some((back, _)) = rejection_of(log, wave) else { return false };
    let returned = log.get(back).and_then(|event| DateTime::parse_from_rfc3339(event.at()).ok());
    returned.is_some_and(|returned| started < returned.with_timezone(&Utc))
}

/// A volta da onda `wave` que espera a rodada: a última entrega que o agente
/// gravou depois do envio que despachou a onda e que nenhuma rodada assumiu.
pub(super) fn pending_return(log: &SpecLog, wave: u64) -> Option<u64> {
    let since = log.last_dispatch_by_wave().get(&wave).copied().unwrap_or_default();
    log.unassumed_returns()
        .into_iter()
        .filter(|e| e.event_type == "delivered" && e.wave() == Some(wave) && e.id > since)
        .map(|e| e.id)
        .max()
}

/// A recusa que segura a volta `last` da onda `wave`, quando é ela a que quem
/// conduz a obra reprovou: a rodada não a assume enquanto ela for a última da
/// onda. O título da recusa vem depois, em [`keep_rejections`].
pub(super) fn held_rejection(log: &SpecLog, wave: u64, last: u64) -> Option<RoundRefusal> {
    let (_, reason) = rejection_of(log, wave).filter(|(back, _)| *back == last)?;
    Some(RoundRefusal::Rejected { wave, reason, title: None })
}

/// Grava cada reprovação que o relatório trouxe (`rejected`, a onda e o
/// motivo), presa à volta que ela reprova: a versão nova do envio da onda,
/// com a volta e o motivo em `rejected`, o consumo do agente medido nos
/// arquivos de conversa de `caller`, como na linha `USAGE`, e sem o Claude
/// Code do envio — a reprovação diz que o agente terminou, e a onda passa a
/// esperar um agente novo, com o Claude Code que mandou a onda aberto ou
/// fechado. A onda sem volta à espera da rodada e a onda cuja volta já entrou
/// num commit são recusadas antes de qualquer gravação. A reprovação repetida
/// da mesma volta com o mesmo motivo não grava nada; com outro motivo, troca
/// só o motivo, e o agente novo que já saiu segue dono da cópia. Quem chama
/// segura a trava do passo do git e leu `log`, do arquivo `path`, sob ela: com
/// algo gravado, `log` é lido de novo. Devolve o que foi gravado e o aviso de
/// cada onda cujo consumo não foi achado.
pub(super) fn record_rejections(
    start: &Path,
    spec: &str,
    path: &Path,
    log: &mut SpecLog,
    rejected: &[(u64, String)],
    caller: Caller<'_>,
    lang: Locale,
) -> Result<(Vec<Value>, Vec<Value>), RoundRefusal> {
    // A última linha de cada onda vale: duas linhas da mesma onda gravariam
    // duas versões a partir da mesma leitura.
    let rejected: BTreeMap<u64, &str> = rejected.iter().map(|(wave, reason)| (*wave, reason.trim())).collect();
    if let Some(wave) = rejected.keys().find(|wave| pending_return(log, **wave).is_none()) {
        let commit = log.visible().into_iter().rfind(|e| e.event_type == "commit" && e.ints("waves").contains(wave));
        let sha = commit.map(|commit| commit.str_field("sha").unwrap_or_default().chars().take(7).collect());
        return Err(match sha {
            Some(sha) => RoundRefusal::RejectedCommitted { wave: *wave, sha },
            None => RoundRefusal::RejectedWithoutReturn { wave: *wave },
        });
    }
    let (mut recorded, mut warnings) = (Vec::new(), Vec::new());
    for (wave, reason) in rejected {
        let Some(back) = pending_return(log, wave) else { continue };
        let before = rejection_of(log, wave).filter(|(id, _)| *id == back);
        if before.as_ref().is_some_and(|(_, said)| said.trim() == reason) {
            continue;
        }
        let mut usage = Usage::default();
        measure_usage(log, caller, std::iter::once((wave, &mut usage)));
        if usage.tokens.is_none() {
            warnings.push(usage_missing(wave, lang));
        }
        let mut extra = usage.fields();
        extra.insert("rejected".into(), json!({ "delivered": back, "reason": reason }));
        let Some(mut draft) = send_revision(log, wave, extra) else { continue };
        // A primeira reprovação da volta tira a onda do agente que a fez: sem
        // Claude Code no envio, ela espera um agente novo.
        if before.is_none() {
            draft.remove("claude_pid");
            draft.remove("claude_started");
        }
        let written = record(start, spec, "send", draft, PhaseWriter::Binary).map_err(RoundRefusal::Refused)?;
        recorded.push(json!({ "wave": wave, "type": "send", "id": written.written.id }));
    }
    if !recorded.is_empty()
        && let Some(fresh) = store::read(path).map_err(RoundRefusal::Refused)?
    {
        *log = fresh;
    }
    Ok((recorded, warnings))
}

/// Grava, para cada volta que quem conduz a obra reprovou (`held`), o motivo
/// dele no arquivo do trecho de conserto da volta ([`fix_file`]), onde o
/// gancho do despacho do agente novo o lê, e dá à recusa de cada uma o título
/// da onda quando ela espera esse agente ([`waves_awaiting_new_agent`]): é o
/// título que a resposta manda despachar. O arquivo é conferido a cada rodada
/// que segura a volta: o motivo mora na spec, e o arquivo só o leva ao agente.
/// A falha de gravação deixa a onda sem o agente novo, e a resposta diz que
/// ele já trabalha.
pub(super) fn keep_rejections(root: &Path, spec: &str, log: &SpecLog, held: &mut [HeldReturn], lang: Locale) {
    for one in held.iter() {
        let RoundRefusal::Rejected { wave, reason, .. } = &one.refusal else { continue };
        let Some(file) = fix_file(root, spec, log, *wave) else { continue };
        let text = translate("round.rejected_fix", lang).replace("{wave}", &wave.to_string()).replace("{reason}", reason.trim());
        if std::fs::read_to_string(&file).ok().as_deref() != Some(text.as_str()) {
            let _ = file.parent().map(std::fs::create_dir_all);
            let _ = std::fs::write(&file, text);
        }
    }
    let awaiting = waves_awaiting_new_agent(root, spec, log);
    for one in held.iter_mut() {
        if let RoundRefusal::Rejected { wave, title, .. } = &mut one.refusal {
            *title = awaiting.contains_key(wave).then(|| wave_title(spec, *wave, lang));
        }
    }
}

/// A frase da volta reprovada, no idioma `lang`: com o título (`title`), a
/// onda espera o agente novo, e a frase manda despachá-lo por ele; sem ele, o
/// agente novo já saiu e trabalha na cópia.
pub(super) fn rejected_message(wave: u64, reason: &str, title: Option<&str>, lang: Locale) -> String {
    let key = if title.is_some() { "round.rejected" } else { "round.rejected_working" };
    translate(key, lang)
        .replace("{wave}", &wave.to_string())
        .replace("{title}", title.unwrap_or_default())
        .replace("{reason}", &without_final_period(reason))
}
