//! Bounded failure metadata, never command/error bodies. Historical counters
//! without this receipt remain unexplained; no cause is backfilled by guessing.
use mustard_core::domain::model::contract::HookInput;
use serde_json::{Value, json};

pub(super) fn record(value: &mut Value, event: &str, input: &HookInput) {
    let response = &input.raw["tool_response"];
    let exit = response["exit_code"].as_i64().or_else(|| response["exitCode"].as_i64());
    if event != "PostToolUseFailure"
        && !(event == "PostToolUse" && (response["isError"] == true || response["is_error"] == true || exit.is_some_and(|code| code != 0)))
    {
        return;
    }
    let call = input.raw["tool_use_id"].as_str().or_else(|| input.raw["call_id"].as_str()).filter(|id| id.len() <= 256).map(super::digest);
    let mut seen = value["failure_calls"].as_array().cloned().unwrap_or_default();
    if call.as_ref().is_some_and(|call| seen.contains(&json!(call))) {
        return;
    }
    if let Some(call) = call {
        seen.push(json!(call));
        if seen.len() > 64 {
            seen.remove(0);
        }
        value["failure_calls"] = json!(seen);
    }
    let error = input.raw["error"].as_str().unwrap_or_default();
    let (category, reason) = if input.raw["is_interrupt"] == true {
        ("interrupted", "tool-interrupted")
    } else if error.contains("[Mustard] Busque por `mcp__mustard__search`") {
        ("blocked", "search-redirect")
    } else if error.contains("[Mustard] mustard.json guarda uma chave secreta") {
        ("blocked", "protected-configuration")
    } else if exit.is_some_and(|code| code != 0) {
        ("failed", "command-exit-nonzero")
    } else {
        ("failed", "tool-error-cause-unknown")
    };
    let tool = input.tool_name.as_deref().filter(|name| super::token(name)).unwrap_or("unknown");
    value["tool_failures"] = json!(value["tool_failures"].as_u64().unwrap_or(0).saturating_add(1));
    value["classified_tool_failures"] = json!(value["classified_tool_failures"].as_u64().unwrap_or(0).saturating_add(1));
    if !value["failure_counts"].is_object() {
        value["failure_counts"] = json!({});
    }
    let count = &mut value["failure_counts"][category];
    *count = json!(count.as_u64().unwrap_or(0).saturating_add(1));
    let mut details = value["recent_tool_failures"].as_array().cloned().unwrap_or_default();
    details.push(json!({"at":value["at"],"event":event,"tool":tool,"category":category,"reason":reason,"exit_code":exit}));
    if details.len() > 20 {
        details.remove(0);
    }
    value["recent_tool_failures"] = json!(details);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinguishes_redirects_commands_interruptions_and_keeps_legacy_unknown() {
        let mut value = json!({"tool_failures":6,"at":"now"});
        for (event, raw) in [
            ("PostToolUseFailure", json!({"tool_use_id":"a","tool_name":"Read","error":"[Mustard] Busque por `mcp__mustard__search` private-key"})),
            ("PostToolUseFailure", json!({"tool_use_id":"a","tool_name":"Read","error":"duplicate"})),
            ("PostToolUse", json!({"tool_use_id":"b","tool_name":"Bash","tool_response":{"exit_code":2,"stderr":"private stderr"}})),
            ("PostToolUseFailure", json!({"tool_use_id":"c","tool_name":"Bash","is_interrupt":true,"error":"private error"})),
            ("PostToolUseFailure", json!({"tool_use_id":"d","tool_name":"Monitor","error":"unknown private content"})),
            ("PostToolUse", json!({"tool_use_id":"e","tool_name":"Grep","tool_response":{"exit_code":0}})),
        ] {
            record(&mut value, event, &serde_json::from_value(raw).unwrap());
        }
        assert_eq!(value["tool_failures"], 10);
        assert_eq!(value["classified_tool_failures"], 4);
        assert_eq!(value["failure_counts"], json!({"blocked":1,"failed":2,"interrupted":1}));
        assert_eq!(value["recent_tool_failures"][1]["exit_code"], 2);
        assert!(!value.to_string().contains("private"));
    }
}
