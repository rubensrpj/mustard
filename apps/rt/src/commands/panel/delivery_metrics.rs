//! Work accepted by the native commit gate, not self-reported completion.
//! Unknown history/usage never becomes a zero cost or a savings percentage.
use mustard_core::domain::spec_events::SpecLog;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn summary(log: &SpecLog, waves: &[Value]) -> Value {
    let mut attempts = BTreeMap::<(u64, u64), Option<u64>>::new();
    for send in log.events.iter().filter(|e| e.event_type == "send") {
        if let Some(wave) = send.wave() {
            let row = attempts
                .entry((wave, log.dispatch_position(send.id)))
                .or_default();
            if let Some(tokens) = send.int("tokens") {
                *row = Some(row.unwrap_or(0).max(tokens));
            }
        }
    }
    // Live current usage can fill this exact attempt; it cannot migrate to an
    // older failed send. A replacement send is an update, not another attempt.
    for wave in waves {
        let id = wave["wave"].as_u64().unwrap_or(0);
        if let Some((_, value)) = attempts
            .iter_mut()
            .filter(|((number, _), _)| *number == id)
            .last()
            && let Some(tokens) = wave["tokens"].as_u64()
        {
            *value = Some(value.unwrap_or(0).max(tokens));
        }
    }
    let accepted: Vec<_> = waves
        .iter()
        .filter(|w| w["status"] == "committed")
        .collect();
    let measured = accepted.iter().filter(|w| w["tokens"].is_number()).count();
    let accepted_ids: BTreeSet<_> = accepted.iter().filter_map(|w| w["wave"].as_u64()).collect();
    let accepted_attempts: Vec<_> = attempts
        .iter()
        .filter(|((wave, _), _)| accepted_ids.contains(wave))
        .map(|(_, tokens)| tokens)
        .collect();
    let accepted_unknown = accepted_attempts
        .iter()
        .filter(|tokens| tokens.is_none())
        .count();
    let accepted_attempt_tokens = accepted_attempts
        .iter()
        .filter_map(|tokens| **tokens)
        .sum::<u64>();
    let current_tokens = accepted
        .iter()
        .filter_map(|w| w["tokens"].as_u64())
        .sum::<u64>();
    let retries = waves
        .iter()
        .filter(|w| {
            attempts
                .keys()
                .filter(|(wave, _)| Some(*wave) == w["wave"].as_u64())
                .count()
                > 1
        })
        .count();
    let unknown_attempts = attempts.values().filter(|value| value.is_none()).count();
    let mut returns = BTreeMap::new();
    for event in log.events.iter().filter(|e| e.event_type == "delivered") {
        if let Some(wave) = event.wave() {
            let dispatch = log
                .events
                .iter()
                .filter(|e| e.id < event.id && e.event_type == "send" && e.wave() == Some(wave))
                .map(|e| log.dispatch_position(e.id))
                .max()
                .unwrap_or(0);
            returns.insert((wave, dispatch), event);
        }
    }
    let zero = returns
        .values()
        .filter(|e| {
            e.fields
                .get("files")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
        })
        .count();
    let proof_runs: Vec<_> = log
        .visible()
        .into_iter()
        .filter(|e| e.event_type == "stage_run" && e.str_field("stage") == Some("criterion-proof"))
        .collect();
    let criteria: BTreeSet<_> = proof_runs
        .iter()
        .filter(|e| e.str_field("result") == Some("pass") && e.int("exit") == Some(0))
        .filter_map(|e| e.int("criterion"))
        .collect();
    let known_attempt_tokens = attempts.values().filter_map(|value| *value).sum::<u64>();
    json!({"accepted_waves":accepted.len(),"accepted_with_usage":measured,"planned_waves":waves.len(),
        "dispatch_attempts":attempts.len(),"waves_with_retries":retries,"delivery_attempts":returns.len(),"zero_change_deliveries":zero,
        "known_attempt_tokens":known_attempt_tokens,"attempts_without_usage":unknown_attempts,
        "accepted_attempts_without_usage":accepted_unknown,
        "tokens_per_accepted_wave":if !accepted.is_empty() && accepted_unknown==0 && measured==accepted.len() {Some(accepted_attempt_tokens/accepted.len() as u64)}else{None},
        "known_accepted_tokens":current_tokens,"verified_criteria":criteria.len(),"native_proof_runs":proof_runs.len(),
        "failed_native_proof_runs":proof_runs.iter().filter(|e|e.str_field("result")==Some("fail")).count(),
        "basis":"accepted means a native commit event; criteria require successful native runs. Zero changes are a review signal, not proof of wasted work.",
        "net_savings_percent":null})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_attempts_count_once_and_unknown_usage_prevents_a_fake_unit_cost() {
        let rows = [
            json!({"id":1,"type":"send","wave":1,"tokens":80}),
            json!({"id":2,"type":"send","wave":1,"replaces":1,"tokens":100}),
            json!({"id":3,"type":"delivered","wave":1,"files":[]}),
            json!({"id":4,"type":"send","wave":1,"resends":2,"tokens":200}),
            json!({"id":5,"type":"delivered","wave":1,"files":["store.rs"]}),
            json!({"id":6,"type":"send","wave":2}),
            json!({"id":7,"type":"stage_run","stage":"criterion-proof","criterion":11,"result":"pass","exit":0}),
        ];
        let log = mustard_core::domain::spec_events::parse_log(
            &rows
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let waves = [
            json!({"wave":1,"status":"committed","tokens":200}),
            json!({"wave":2,"status":"committed","tokens":null}),
        ];
        let out = summary(&log, &waves);
        assert_eq!(out["dispatch_attempts"], 3);
        assert_eq!(out["waves_with_retries"], 1);
        assert_eq!(out["known_attempt_tokens"], 300);
        assert_eq!(out["tokens_per_accepted_wave"], Value::Null);
        assert_eq!(out["zero_change_deliveries"], 1);
        assert_eq!(out["verified_criteria"], 1);
        assert_eq!(out["net_savings_percent"], Value::Null);
    }
}
