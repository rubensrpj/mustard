//! Parsing the explicit conductor report, without filesystem effects.
use super::{DELIVERED_LINE, FALSE_POSITIVE_LINE, PAUSED_LINE, PLAN_REFUSED_LINE, Report, RoundRefusal, USAGE_LINE, Usage, VERDICT_LINE, rejected_lines};
use serde_json::{Map, Value};

pub(crate) fn tagged<'a>(raw: &'a str, tag: &str) -> Vec<&'a str> {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut out = Vec::new();
    let mut rest = raw;
    while let Some(at) = rest.find(&open) {
        let after = &rest[at + open.len()..];
        let Some(end) = after.find(&close) else { break };
        out.push(after[..end].trim());
        rest = &after[end + close.len()..];
    }
    out
}

/// O objeto JSON de uma linha, com a onda dela, quando ela traz uma.
pub(crate) fn line_object(body: &str, line: &'static str) -> Result<(Option<u64>, Map<String, Value>), RoundRefusal> {
    let parsed: Value = serde_json::from_str(body).map_err(|e| RoundRefusal::BadReport { detail: format!("{line}: {e}") })?;
    let Value::Object(fields) = parsed else {
        return Err(RoundRefusal::BadReport { detail: format!("{line}: {body}") });
    };
    Ok((fields.get("wave").and_then(Value::as_u64), fields))
}

/// As linhas do relatório que o orquestrador passa: a marca de que a onda
/// terminou (`USAGE`), a pausa (`PAUSED`), a reprovação de uma volta, com o
/// motivo (`REJECTED`), a recusa da mudança de plano de uma onda, com o
/// motivo (`PLAN_REFUSED`), e o falso positivo da conferência depois da onda,
/// com o motivo (`FALSE_POSITIVE`), como vieram. A entrega e o veredito não
/// vêm aqui: moram na spec, e a linha `DELIVERED` ou `VERDICT` colada no
/// relatório é recusada. O texto sem nenhuma dessas linhas não se entende. O
/// resto do texto não é lido.
pub(crate) fn parse_report(raw: &str) -> Result<Report, RoundRefusal> {
    if !tagged(raw, DELIVERED_LINE).is_empty() || !tagged(raw, VERDICT_LINE).is_empty() {
        return Err(RoundRefusal::ReturnLine);
    }
    let usage_bodies = tagged(raw, USAGE_LINE);
    let paused_bodies = tagged(raw, PAUSED_LINE);
    let mut reviewed = Vec::new();
    for body in tagged(raw, "REVIEWED") {
        let (wave, fields) = line_object(body, "REVIEWED")?;
        let wave = wave.ok_or(RoundRefusal::LineField { line: "REVIEWED", field: "wave" })?;
        let text = |key| fields.get(key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
        // Sem a impressão, o aceite é o da volta pendente que quem conduz viu.
        let fingerprint = text("fingerprint").map(str::to_owned);
        let note = text("note").ok_or(RoundRefusal::LineField { line: "REVIEWED", field: "note" })?;
        reviewed.push(super::super::review::Approval { wave, fingerprint, note: note.into() });
    }
    let rejected = rejected_lines(raw)?;
    let mut plan_refused = Vec::new();
    for body in tagged(raw, PLAN_REFUSED_LINE) {
        let (wave, fields) = line_object(body, PLAN_REFUSED_LINE)?;
        let wave = wave.ok_or(RoundRefusal::LineField { line: PLAN_REFUSED_LINE, field: "wave" })?;
        let reason = fields.get("reason").and_then(Value::as_str).map(str::trim).filter(|said| !said.is_empty());
        let reason = reason.ok_or(RoundRefusal::LineField { line: PLAN_REFUSED_LINE, field: "reason" })?;
        plan_refused.push((wave, reason.to_string()));
    }
    let mut false_positive = Vec::new();
    for body in tagged(raw, FALSE_POSITIVE_LINE) {
        let (wave, fields) = line_object(body, FALSE_POSITIVE_LINE)?;
        let wave = wave.ok_or(RoundRefusal::LineField { line: FALSE_POSITIVE_LINE, field: "wave" })?;
        let reason = fields.get("reason").and_then(Value::as_str).map(str::trim).filter(|said| !said.is_empty());
        let reason = reason.ok_or(RoundRefusal::LineField { line: FALSE_POSITIVE_LINE, field: "reason" })?;
        false_positive.push((wave, reason.to_string()));
    }
    let unmarked = usage_bodies.is_empty()
        && paused_bodies.is_empty()
        && rejected.is_empty()
        && reviewed.is_empty()
        && plan_refused.is_empty()
        && false_positive.is_empty();
    if unmarked && !raw.trim().is_empty() {
        let shown: String = raw.trim().chars().take(80).collect();
        return Err(RoundRefusal::BadReport { detail: shown });
    }
    // Da linha `USAGE` vale só a onda: o consumo a rodada mede nos arquivos
    // de conversa da plataforma, e o número que ainda vier na linha é
    // ignorado.
    let mut usage = Vec::new();
    for body in usage_bodies {
        let (wave, _) = line_object(body, USAGE_LINE)?;
        let wave = wave.ok_or(RoundRefusal::LineField { line: USAGE_LINE, field: "wave" })?;
        usage.push((wave, Usage::default()));
    }
    let mut paused = Vec::new();
    for body in paused_bodies {
        let (wave, _) = line_object(body, PAUSED_LINE)?;
        let wave = wave.ok_or(RoundRefusal::LineField { line: PAUSED_LINE, field: "wave" })?;
        paused.push(wave);
    }
    Ok(Report { waves: Vec::new(), verdicts: Vec::new(), paused, usage, rejected, reviewed, plan_refused, false_positive })
}
