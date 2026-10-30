//! As etapas de uma spec no painel. Duas fontes as guardam: a spec, onde fica a
//! execução com a saída, e o arquivo de operações, onde o cronômetro da rodada
//! deixa o tempo de cada etapa. A compilação vermelha da rodada está nas duas,
//! e a etapa sai uma vez só.

use mustard_core::domain::spec_events::SpecEvent;

use crate::commands::spec_events::conversation::time_of;

/// As etapas da spec (`in_spec`) e do arquivo de operações (`observed`), na
/// ordem da hora. A execução que as duas fontes guardam fica com a da spec,
/// que é a que traz a saída.
pub(super) fn listed<'a>(in_spec: &[&'a SpecEvent], observed: &[&'a SpecEvent]) -> Vec<&'a SpecEvent> {
    let runs: Vec<&SpecEvent> = in_spec.iter().copied().filter(|event| event.event_type == "stage_run").collect();
    let timings = observed.iter().copied().filter(|event| event.event_type == "stage_run" && !runs.iter().any(|run| timed(event, run)));
    let mut stages: Vec<&SpecEvent> = runs.iter().copied().chain(timings).collect();
    stages.sort_by_key(|event| time_of(event));
    stages
}

/// O cronômetro `timing` mediu a execução `run`: é a mesma etapa, do mesmo
/// processo, e a spec a gravou dentro do tempo que o cronômetro contou. O
/// número do processo sozinho não basta, porque o sistema o devolve a outro
/// processo mais tarde.
fn timed(timing: &SpecEvent, run: &SpecEvent) -> bool {
    let attempt = timing.int("attempt");
    if attempt.is_none() || attempt != run.int("attempt") || timing.str_field("phase") != run.str_field("phase") || timing.str_field("stage") != run.str_field("stage") {
        return false;
    }
    let Some((end, start)) = time_of(timing).zip(time_of(run)) else {
        return false;
    };
    // A hora gravada pode ter só a precisão de um segundo, que cabe além do tempo medido.
    let window = i64::try_from(timing.int("ms").unwrap_or(0)).unwrap_or(i64::MAX).saturating_mul(1_000_000).saturating_add(1_000_000_000);
    (0..=window).contains(&(end - start))
}

#[cfg(test)]
mod tests {
    use mustard_core::domain::spec_events::parse_log;
    use serde_json::{Value, json};

    use super::*;

    fn log(rows: &[Value]) -> mustard_core::domain::spec_events::SpecLog {
        parse_log(&rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n"))
    }

    fn build(id: u64, at: &str, attempt: u64, ms: u64) -> Value {
        json!({"v":1,"id":id,"at":at,"type":"stage_run","phase":"delivery","stage":"runtime-build","result":"fail","ms":ms,"attempt":attempt,"author":"binary"})
    }

    /// O cronômetro que mediu a execução gravada na spec some; o de outro
    /// processo, ou o do mesmo número de processo numa rodada de outra hora,
    /// continua na lista.
    #[test]
    fn a_stopwatch_leaves_the_list_only_for_the_run_it_timed() {
        let in_spec = log(&[build(1, "2026-10-28T10:00:00-03:00", 77, 3000)]);
        let observed = log(&[
            build(1, "2026-10-28T10:00:03-03:00", 77, 4000),
            build(2, "2026-10-28T10:00:03-03:00", 78, 4000),
            build(3, "2026-10-28T11:30:00-03:00", 77, 800),
        ]);
        let (spec_side, ops_side) = (in_spec.visible(), observed.visible());
        let kept: Vec<(&str, Option<u64>)> = listed(&spec_side, &ops_side).iter().map(|event| (event.at(), event.int("attempt"))).collect();
        assert_eq!(
            kept,
            [("2026-10-28T10:00:00-03:00", Some(77)), ("2026-10-28T10:00:03-03:00", Some(78)), ("2026-10-28T11:30:00-03:00", Some(77))],
            "a execução da spec fica, e só o cronômetro que a mediu sai"
        );
    }
}
