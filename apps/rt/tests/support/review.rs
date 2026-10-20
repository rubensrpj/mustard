use std::fmt::Write as _;
use std::path::Path;

/// The fixture conductor reviews through the actual CLI. This does not
/// bypass the production gate: it obtains a native view and accepts it in
/// the subsequent report without copying the fingerprint — the binary binds
/// the acceptance to the viewed fingerprint of the pending return.
pub fn review_report(root: &Path, spec: &str, previous: &str, view: impl Fn(u64) -> serde_json::Value) -> String {
    let Some(path) = mustard_core::io::spec_events::spec_file(root, spec).ok() else { return previous.into() };
    let Some(log) = mustard_core::io::spec_events::read(&path).ok().flatten() else { return previous.into() };
    let mut report = previous.to_string();
    for event in log.unassumed_returns().into_iter().filter(|event| event.event_type == "delivered") {
        let Some(wave) = event.wave() else { continue };
        if event.fields.get("files").and_then(serde_json::Value::as_array).is_none_or(Vec::is_empty) { continue }
        let out = view(wave);
        if out["fingerprint"].as_str().is_none() { continue }
        let approved = serde_json::json!({"wave":wave,"note":"Fixture conductor reviewed diff, rules and tests"});
        let _ = write!(report, "\n<REVIEWED>{approved}</REVIEWED>");
    }
    report
}
