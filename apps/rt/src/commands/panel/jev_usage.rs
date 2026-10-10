//! Billing derives from physical attempts; logical requests and cache reuse
//! never count as extra payment. Historical/incomplete records stay unknown.
use super::{json, Value, Path};

fn rows(path: std::path::PathBuf) -> Vec<Value> {
    std::fs::read_to_string(path)
        .ok()
        .as_deref()
        .into_iter()
        .flat_map(str::lines)
        .map(|line| serde_json::from_str(line).unwrap_or_else(|_| json!({"result":"unreadable"})))
        .collect()
}
pub(super) struct Ledger {
    logical: Vec<Value>,
    attempts: Vec<Value>,
}
impl Ledger {
    pub(super) fn read(root: &Path) -> Self {
        Self { logical: rows(root.join(".claude/judgements/requests.ndjson")), attempts: rows(root.join(".claude/judgements/attempts.ndjson")) }
    }
    pub(super) fn project(&self) -> Value {
        summarize(&self.logical, &self.attempts)
    }
    pub(super) fn spec(&self, name: &str) -> Value {
        let selected = self.attempts.iter().filter(|row| row["spec"] == name).cloned().collect::<Vec<_>>();
        let missing_attribution = self.attempts.iter().filter(|row| row["spec"].as_str().is_none()).count()
            + self.logical.iter().filter(|row| row["physical_tracking"] != true).count();
        let mut summary = summarize(&[], &selected);
        summary["unattributed_project_attempts"] = json!(missing_attribution);
        summary["attribution"] = json!("explicit-spec-only");
        if missing_attribution > 0 {
            summary["cost_micro_usd"] = Value::Null;
        }
        summary
    }
}
fn summarize(logical: &[Value], attempts: &[Value]) -> Value {
    let legacy = logical.iter().filter(|row| row["physical_tracking"] != true).collect::<Vec<_>>();
    let native = logical.iter().filter(|row| row["physical_tracking"] == true).collect::<Vec<_>>();
    let missing = native.iter().any(|row| {
        let expected = row["attempts"].as_u64();
        let actual = attempts.iter().filter(|attempt| attempt["request_id"] == row["request_id"]).count() as u64;
        expected.is_none() || actual < expected.unwrap_or(1)
    });
    let legacy_requests: Option<u64> = legacy.iter().map(|row| row["attempts"].as_u64()).sum();
    let malformed = attempts.iter().any(|row| row["request_id"].as_str().is_none());
    let requests = if missing || malformed { None } else { legacy_requests.map(|legacy| legacy + attempts.len() as u64) };
    let known_requests = attempts.len() as u64 + legacy.iter().map(|row| row["attempts"].as_u64().unwrap_or(1)).sum::<u64>();
    let tokens = attempts.iter().chain(legacy.iter().copied()).filter_map(|row| row["input_tokens"].as_u64()).sum::<u64>();
    let unknown = attempts.iter().filter(|row| row["input_tokens"].as_u64().is_none()).count()
        + legacy.iter().filter(|row| row["input_tokens"].as_u64().is_none() || row["attempts"].as_u64() != Some(1)).count();
    json!({"physical_requests":requests,"known_physical_requests":known_requests,"judgements":logical.len(),
        "known_input_tokens":tokens,"requests_with_unknown_usage":unknown,
        "cost_micro_usd":if unknown==0 && !missing && known_requests>0 {Some((tokens as f64*crate::shared::jev::PRICE_PER_MILLION_INPUT_TOKENS).round() as u64)}else{None},
        "cost_basis":"table-estimate","origin":"physical-attempt-log"})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_retry_counts_each_physical_attempt_without_double_billing_the_logical_success() {
        let logical = vec![json!({"request_id":"r","attempts":2,"physical_tracking":true,"input_tokens":1000})];
        let attempts = vec![json!({"request_id":"r","attempt":1,"status":429,"input_tokens":null}), json!({"request_id":"r","attempt":2,"input_tokens":1000})];
        let measured = summarize(&logical, &attempts);
        assert_eq!(measured["physical_requests"], 2);
        assert_eq!(measured["known_input_tokens"], 1000);
        assert_eq!(measured["requests_with_unknown_usage"], 1);
        assert!(measured["cost_micro_usd"].is_null());
        assert!(summarize(&logical, &attempts[..1])["physical_requests"].is_null());
    }
}
