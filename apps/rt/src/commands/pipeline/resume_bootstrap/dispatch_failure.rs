//! Dispatch-failure replay: render the most recent unrecovered dispatch
//! failure (already TTL-filtered upstream) into the DTO's JSON shape with a
//! freshly computed `ageMs`.

use mustard_core::domain::model::event::PipelineDispatchFailurePayload;
use serde_json::json;

/// Render the dispatch failure payload as JSON, including `ageMs`.
pub(super) fn render_dispatch_failure(fail: &PipelineDispatchFailurePayload) -> serde_json::Value {
    let now_ms = i64::try_from(mustard_core::time::now_unix_millis() as u128).unwrap_or(i64::MAX);
    let age_ms = fail
        .at
        .as_deref()
        .and_then(mustard_core::time::parse_iso_millis)
        .map_or(0, |at_ms| now_ms - at_ms);
    json!({
        "at": fail.at.clone().unwrap_or_default(),
        "ageMs": age_ms,
        "agentType": fail.agent_type.clone().unwrap_or_default(),
        "description": fail.description.clone().unwrap_or_default(),
        "prompt": fail.prompt.clone().unwrap_or_default(),
    })
}
