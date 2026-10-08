//! Exclude internal, returned and purged records from secret checks.
use mustard_core::domain::spec_events::{CUT_LINE_TYPE, Hidden, SpecEvent, SpecLog};
use serde_json::Value;
const LEFT_OUT: &[&str] = &["injection", "hook", "call"];

fn never_copied(event: &SpecEvent, log_hidden: &std::collections::BTreeMap<u64, Hidden>) -> bool {
    LEFT_OUT.contains(&event.event_type.as_str())
        || event.event_type == CUT_LINE_TYPE
        || matches!(log_hidden.get(&event.id), Some(Hidden::Purged { .. }))
        || event.returned()
}

pub(crate) fn withheld(log: &SpecLog) -> Vec<String> {
    let codes = log.codes();
    let hidden = log.hidden();
    let mut out: Vec<String> = Vec::new();
    for event in log.events.iter().filter(|e| !never_copied(e, &hidden)) {
        let mut fields = event.fields.clone();
        fields.remove("search");
        if holds_secret(&Value::Object(fields)) {
            let code = codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string());
            if !out.contains(&code) {
                out.push(code);
            }
        }
    }
    out
}

fn holds_secret(value: &Value) -> bool {
    match value {
        Value::String(text) => !crate::shared::secret::secret_excerpts(text).is_empty(),
        Value::Array(items) => items.iter().any(holds_secret),
        Value::Object(map) => map.values().any(holds_secret),
        _ => false,
    }
}
