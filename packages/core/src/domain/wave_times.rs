//! O tempo que cada onda levou num projeto, da saída ao commit, lido da
//! história das specs dele. A conta pura, sem disco e sem relógio.
//!
//! A onda sai no primeiro envio dela, pela versão que a despachou
//! ([`SpecLog::dispatch_position`]), e termina no primeiro commit que a leva
//! depois disso. As voltas, os reenvios e os consertos do meio entram no
//! tempo dela. A onda que saiu e ainda não tem commit não é medida.

use chrono::DateTime;
use serde::Serialize;

use crate::domain::spec_events::SpecLog;

/// A medida das ondas de um projeto, em minutos: quantas foram medidas, a
/// mais curta, a do meio e a mais longa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WaveTimes {
    pub measured: usize,
    pub shortest: u64,
    pub median: u64,
    pub longest: u64,
}

/// Os minutos de cada onda de `log` que saiu e entrou num commit, na ordem
/// do número da onda. O envio que não é de onda (o do revisor) não conta como
/// saída.
#[must_use]
pub fn wave_minutes(log: &SpecLog) -> Vec<u64> {
    let mut first_out: std::collections::BTreeMap<u64, u64> = std::collections::BTreeMap::new();
    for send in log.events.iter().filter(|e| e.event_type == "send" && e.str_field("role").is_none_or(|role| role == "wave")) {
        if let Some(wave) = send.wave() {
            let at = log.dispatch_position(send.id);
            first_out.entry(wave).and_modify(|first| *first = (*first).min(at)).or_insert(at);
        }
    }
    let commits: Vec<_> = log.visible().into_iter().filter(|e| e.event_type == "commit").collect();
    first_out
        .into_iter()
        .filter_map(|(wave, out)| {
            let left = DateTime::parse_from_rfc3339(log.get(out)?.at()).ok()?;
            let commit = commits.iter().find(|commit| commit.id > out && commit.ints("waves").contains(&wave))?;
            let landed = DateTime::parse_from_rfc3339(commit.at()).ok()?;
            let seconds = u64::try_from((landed - left).num_seconds()).ok()?;
            Some((seconds + 30) / 60)
        })
        .collect()
}

/// A medida das ondas de todas as specs `logs` de um projeto; `None` quando
/// nenhuma onda foi medida. Com um número par de ondas, a do meio é a de
/// cima das duas centrais.
#[must_use]
pub fn wave_times<'a>(logs: impl IntoIterator<Item = &'a SpecLog>) -> Option<WaveTimes> {
    let mut minutes: Vec<u64> = logs.into_iter().flat_map(wave_minutes).collect();
    minutes.sort_unstable();
    Some(WaveTimes { measured: minutes.len(), shortest: *minutes.first()?, median: minutes[minutes.len() / 2], longest: *minutes.last()? })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::parse_log;

    fn log(lines: &[&str]) -> SpecLog {
        parse_log(&lines.join("\n"))
    }

    /// Cada onda conta do primeiro envio ao commit que a leva, com o reenvio
    /// do meio; a versão do envio que só soma o consumo não muda a saída, e a
    /// onda sem commit fica de fora.
    #[test]
    fn each_wave_counts_from_its_first_dispatch_to_its_commit() {
        let spec = log(&[
            r#"{"v":1,"id":1,"at":"2026-10-01T10:00:00-03:00","type":"send","wave":1,"role":"wave","text":"Pedido."}"#,
            r#"{"v":1,"id":2,"at":"2026-10-01T10:30:00-03:00","type":"send","wave":1,"replaces":1,"tokens":10}"#,
            r#"{"v":1,"id":3,"at":"2026-10-01T10:40:00-03:00","type":"send","wave":1,"role":"wave","text":"Reenvio."}"#,
            r#"{"v":1,"id":4,"at":"2026-10-01T10:00:00-03:00","type":"send","wave":2,"role":"wave","text":"Pedido."}"#,
            r#"{"v":1,"id":5,"at":"2026-10-01T10:45:00-03:00","type":"commit","waves":[1],"sha":"abc"}"#,
            r#"{"v":1,"id":6,"at":"2026-10-01T10:00:00-03:00","type":"send","wave":3,"role":"wave","text":"Pedido."}"#,
            r#"{"v":1,"id":7,"at":"2026-10-01T10:20:00-03:00","type":"commit","waves":[3],"sha":"def"}"#,
        ]);

        assert_eq!(wave_minutes(&spec), vec![45, 20]);
        let times = wave_times([&spec, &log(&[])]);
        assert_eq!(times, Some(WaveTimes { measured: 2, shortest: 20, median: 45, longest: 45 }));
    }
}
