//! Parsing the explicit conductor report, without filesystem effects.
use super::{DELIVERED_LINE, PAUSED_LINE, Report, RoundRefusal, USAGE_LINE, Usage, VERDICT_LINE, rejected_lines};
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
/// terminou (`USAGE`), a pausa (`PAUSED`) e a reprovação de uma volta, com o
/// motivo (`REJECTED`), como vieram. A entrega e o veredito não
/// vêm aqui: moram na spec, e a linha `DELIVERED` ou `VERDICT` colada no
/// relatório é recusada. O texto sem nenhuma dessas linhas não se entende. O
/// resto do texto não é lido.
pub(crate) fn parse_report(raw: &str) -> Result<Report, RoundRefusal> {
    if !tagged(raw, DELIVERED_LINE).is_empty() || !tagged(raw, VERDICT_LINE).is_empty() {
        return Err(RoundRefusal::ReturnLine);
    }
    let usage_bodies = tagged(raw, USAGE_LINE);
    let paused_bodies = tagged(raw, PAUSED_LINE);
    let rejected = rejected_lines(raw)?;
    let unmarked = usage_bodies.is_empty() && paused_bodies.is_empty() && rejected.is_empty();
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
    Ok(Report { waves: Vec::new(), verdicts: Vec::new(), paused, usage, rejected })
}
