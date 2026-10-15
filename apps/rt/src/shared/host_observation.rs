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

mod failures;
mod transcripts;
pub(crate) use transcripts::tool_binding;
pub(crate) use transcripts::wave_usage;

/// A CLI call must not inherit another host's most recent session by mtime.
pub(crate) fn caller_host(root: &Path, session: Option<&str>) -> &'static str {
    if let Some(session) = session {
        let codex = root.join(".claude/mustard/host-observations").join(format!("codex-{}.json", digest(session)));
        let claude = root.join(".claude/mustard/host-observations").join(format!("claude-{}.json", digest(session)));
        if codex.is_file() && !claude.is_file() { return "codex"; }
    }
    if crate::shared::spec_state::is_codex_session(session) { "codex" } else { "claude" }
}

fn digest(value: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(value.as_bytes());
    hash.hex_digest()
}

/// Use the exact session, even after a background command is reparented.
pub(crate) fn caller_process(root: &Path, session: Option<&str>) -> Option<(u32, u64)> {
    let session = session?;
    let host = caller_host(root, Some(session));
    let path = root.join(".claude/mustard/host-observations").join(format!("{host}-{}.json", digest(session)));
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 65_536 { return None; }
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let pid = u32::try_from(value["host_pid"].as_u64()?).ok()?;
    let started = value["host_started"].as_u64()?;
    crate::commands::flow::stuck::process_alive(pid, started).then_some((pid, started))
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
    transcripts::observe(root,host,event,input);
    let receipt=super::tool_observations::capture(root,host,event,input);
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
    if let Some((pid, started)) = crate::commands::flow::stuck::sender_process() {
        value["host_pid"] = json!(pid);
        value["host_started"] = json!(started);
    }
    value["events"][event] = json!(
        value["events"][event]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(1)
    );
    value["last_event"] = json!(event);
    if let Some(receipt)=receipt {
        let field=if receipt.is_ok() {"tool_receipt_events"} else {"tool_receipt_failures"};
        value[field]=json!(value[field].as_u64().unwrap_or(0).saturating_add(1));
        if let Err(reason)=receipt {value["last_tool_receipt_error"]=json!(reason);}
    }
    if let Some(cwd)=input.cwd.as_deref() {
        let identity=digest(cwd);
        if value["working_directory_id"].as_str().is_some_and(|previous|previous!=identity) {
            value["working_directory_changes"]=json!(value["working_directory_changes"].as_u64().unwrap_or(0).saturating_add(1));
        }
        value["working_directory_id"]=json!(identity);
        value["workspace_id"]=json!(digest(&root.to_string_lossy()));
    }
    if (event=="SessionStart" || event=="SessionEnd") && let Some(reason)=input.raw.get("source").or_else(||input.raw.get("reason")).and_then(Value::as_str).filter(|s|token(s)) {
        value["session_transition"]=json!(reason);
    }
    if let Some(model) = input
        .raw
        .get("model")
        .and_then(Value::as_str)
        .filter(|s| token(s))
    {
        value["observed_model"] = json!(model);
    }
    if let Some(agent) = input
        .agent_type.as_deref()
        .filter(|s| token(s))
    {
        value["observed_agent_type"] = json!(agent);
    }
    failures::record(&mut value, event, input);
    if host == "codex"
        && let Some(path) = input.raw.get("transcript_path").and_then(Value::as_str)
        && let Some(usage) = codex_usage(Path::new(path))
    {
        value["usage"] = usage;
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
                && let Ok(mut value) = serde_json::from_str::<Value>(&text)
            {
                value.as_object_mut().map(|fields| fields.remove("failure_calls"));
                value["unexplained_tool_failures"] =
                    json!(value["tool_failures"].as_u64().unwrap_or(0).saturating_sub(value["classified_tool_failures"].as_u64().unwrap_or(0)));
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
    #[test]
    fn missing_rollouts_preserve_observed_usage_and_workspace_transitions_do_not_store_paths() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();
        let transcript=root.join("rollout.jsonl");
        std::fs::write(&transcript,json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":20,"total_tokens":120}}}}).to_string()).unwrap();
        let first:HookInput=serde_json::from_value(json!({"session_id":"s","cwd":"/private/first","source":"startup","transcript_path":transcript})).unwrap();
        observe(root,"codex","SessionStart",&first);
        let next:HookInput=serde_json::from_value(json!({"session_id":"s","cwd":"/private/second","transcript_path":"/missing/rollout"})).unwrap();
        observe(root,"codex","PostToolUse",&next);
        let report=snapshot(root);let observed=&report["observations"][0];
        assert_eq!(observed["usage"]["total_tokens"],120);
        assert_eq!(observed["working_directory_changes"],1);
        assert_eq!(observed["session_transition"],"startup");
        assert_eq!(caller_host(root,Some("s")),"codex");
        assert_eq!(caller_host(root,Some("unrelated")),"claude");
        assert!(!report.to_string().contains("/private/"));
    }
}
