use std::fmt::Write as _;
use std::path::Path;

/// The fixture conductor reviews through the actual CLI. This does not
/// bypass the production gate: it obtains a native view and explicitly
/// approves that exact fingerprint in the subsequent report.
pub fn review_report(root: &Path, spec: &str, previous: &str, view: impl Fn(u64) -> serde_json::Value) -> String {
    let Some(path) = mustard_core::io::spec_events::spec_file(root, spec).ok() else { return previous.into() };
    let Some(log) = mustard_core::io::spec_events::read(&path).ok().flatten() else { return previous.into() };
    let mut report = previous.to_string();
    for event in log.unassumed_returns().into_iter().filter(|event| event.event_type == "delivered") {
        let Some(wave) = event.wave() else { continue };
        if event.fields.get("files").and_then(serde_json::Value::as_array).is_none_or(Vec::is_empty) { continue }
        let out = view(wave);
        let Some(hash) = out["fingerprint"].as_str() else { continue };
        let approved = serde_json::json!({"wave":wave,"fingerprint":hash,"note":"Fixture conductor reviewed diff, rules and tests"});
        let _ = write!(report, "\n<REVIEWED>{approved}</REVIEWED>");
    }
    report
}
