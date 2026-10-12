//! Host events prove hook execution, not hook trust settings or model savings.
//! Persist metadata only. Codex rollout usage is an explicitly versioned,
//! best-effort adapter; an unrecognized format remains unknown.
use mustard_core::{
    domain::model::contract::HookInput,
    io::{fs::lock::LockedFile, sha256::Sha256},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

fn digest(value: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(value.as_bytes());
    hash.hex_digest()
}
fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
}

pub(crate) fn observe(root: &Path, host: &str, event: &str, input: &HookInput) {
    let Some(session) = input.session_id.as_deref().filter(|s| !s.is_empty()) else {
        return;
    };
    if !["claude", "codex"].contains(&host) {
        return;
    }
    let path = root
        .join(".claude/mustard/host-observations")
        .join(format!("{host}-{}.json", digest(session)));
    // Do not follow a planted output directory outside the project.
    let mut ancestor = path.clone();
    while !ancestor.exists() {
        if !ancestor.pop() {
            return;
        }
    }
    if !ancestor
        .canonicalize()
        .ok()
        .zip(root.canonicalize().ok())
        .is_some_and(|(p, r)| p.starts_with(r))
    {
        return;
    }
    let Ok(Some(mut held)) = LockedFile::exclusive_if_free(&path) else {
        return;
    };
    let mut value: Value = held
        .read_to_string()
        .ok()
        .filter(|s| s.len() <= 65_536)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({"host":host,"events":{},"tool_failures":0}));
    value["at"] = json!(chrono::Utc::now().to_rfc3339());
    value["events"][event] = json!(
        value["events"][event]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(1)
    );
    value["last_event"] = json!(event);
    if let Some(model) = input
        .raw
        .get("model")
        .and_then(Value::as_str)
        .filter(|s| token(s))
    {
        value["observed_model"] = json!(model);
    }
    if let Some(agent) = input
        .raw
        .get("agent_type")
        .and_then(Value::as_str)
        .filter(|s| token(s))
    {
        value["observed_agent_type"] = json!(agent);
    }
    if event == "PostToolUse"
        && input
            .raw
            .pointer("/tool_response/isError")
            .or_else(|| input.raw.pointer("/tool_response/is_error"))
            == Some(&json!(true))
    {
        value["tool_failures"] = json!(
            value["tool_failures"]
                .as_u64()
                .unwrap_or(0)
                .saturating_add(1)
        );
    }
    if host == "codex"
        && let Some(path) = input.raw.get("transcript_path").and_then(Value::as_str)
    {
        value["usage"] = codex_usage(Path::new(path)).unwrap_or(Value::Null);
    }
    value["usage_status"] = json!(if value["usage"].is_object() {
        "observed-versioned-rollout"
    } else {
        "unknown"
    });
    let _ = held.replace(value.to_string().as_bytes());
}

fn codex_usage(path: &Path) -> Option<Value> {
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(131_072)))
        .ok()?;
    let mut bytes = Vec::new();
    file.take(131_072).read_to_end(&mut bytes).ok()?;
    for line in String::from_utf8_lossy(&bytes).lines().rev() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if event["type"] != "event_msg" || event["payload"]["type"] != "token_count" {
            continue;
        }
        return usage(&event["payload"]["info"]["total_token_usage"]);
    }
    None
}

fn usage(value: &Value) -> Option<Value> {
    let input = value["input_tokens"].as_u64()?;
    let output = value["output_tokens"].as_u64()?;
    let cached = value["cached_input_tokens"].as_u64()?;
    let total = value["total_tokens"].as_u64()?;
    if cached > input || input.checked_add(output)? != total {
        return None;
    }
    Some(
        json!({"adapter":"codex-event-msg-token-count-v1","input_tokens":input,"cached_input_tokens":cached,
        "output_tokens":output,"total_tokens":total,"billing":"unknown; subscription cost not inferred",
        "scope":"this transcript only; cached input is a subset, not an additional charge"}),
    )
}

pub(crate) fn snapshot(root: &Path) -> Value {
    let mut observations = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root.join(".claude/mustard/host-observations")) {
        let mut latest = Vec::new();
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_file())
                && let Ok(metadata) = entry.metadata()
                && metadata.len() <= 65_536
                && let Ok(modified) = metadata.modified()
            {
                latest.push((modified, entry.path()));
                latest.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
                latest.truncate(8);
            }
        }
        for (_, path) in latest {
            if let Ok(text) = mustard_core::io::fs::lock::read_shared(&path)
                && text.len() <= 65_536
                && let Ok(value) = serde_json::from_str::<Value>(&text)
            {
                observations.push(value);
            }
        }
    }
    observations.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));
    observations.truncate(8);
    json!({"observations":observations,"hook_activation":"observed events only; installation alone is insufficient",
        "net_savings":null,"usage_contract":"unknown formats remain unknown; no Claude/Codex cross-attribution"})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_usage_does_not_double_count_cached_input_or_guess_unknown_shapes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let good = json!({"input_tokens":100,"cached_input_tokens":80,"output_tokens":20,"total_tokens":120});
        std::fs::write(&path,format!("private prompt\n{}\n",json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":good}}}))).unwrap();
        assert_eq!(codex_usage(&path).unwrap()["total_tokens"], 120);
        let mut invalid = good;
        invalid["cached_input_tokens"] = json!(101);
        assert!(usage(&invalid).is_none());
        std::fs::write(&path, "{\"usage\":{\"tokens\":120}}\n").unwrap();
        assert!(codex_usage(&path).is_none());
    }
    #[test]
    fn hook_observation_stores_metadata_and_separates_hosts() {
        let dir = tempfile::tempdir().unwrap();
        let input: HookInput = serde_json::from_value(
            json!({"session_id":"private-session","model":"gpt-6-luna","prompt":"private prompt"}),
        )
        .unwrap();
        observe(dir.path(), "codex", "UserPromptSubmit", &input);
        observe(dir.path(), "claude", "SessionStart", &input);
        let report = snapshot(dir.path());
        assert_eq!(report["observations"].as_array().unwrap().len(), 2);
        assert!(!report.to_string().contains("private"));
        assert!(report["net_savings"].is_null());
    }
}
