//! Project-local Codex adapter installation. Merge host configuration without
//! editing global config, manual skills, comments or trusting hooks for a user.
use std::collections::BTreeMap;
use std::path::Path;
use serde_json::{Value, json};
use crate::io::{fs, sha256::Sha256};

fn hash(bytes: &[u8]) -> String { let mut hash = Sha256::new(); hash.update(bytes); hash.hex_digest() }

fn read_object(path: &Path) -> Result<Value, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let value: Value = serde_json::from_slice(&bytes).map_err(|_| "codex-install-invalid-json")?;
            if !value.is_object() { return Err("codex-install-object-required".into()); }
            Ok(value)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.to_string()),
    }
}

pub fn install(root: &Path) -> Result<Value, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    // Keep project-local config inside this root even when a user has linked
    // an unrelated config/skills directory into the checkout.
    for directory in [".codex", ".codex/agents", ".codex/config.toml", ".codex/hooks.json", ".codex/.mustard-install.json", ".codex/.mustard-install.lock", "mustard.json", ".agents", ".agents/skills", ".agents/skills/mustard"] {
        let path = root.join(directory);
        if path.symlink_metadata().is_ok() && !path.canonicalize().map_err(|e|e.to_string())?.starts_with(&root) {
            return Err("codex-install-linked-directory-outside-project".into());
        }
    }
    fs::create_dir_all(root.join(".codex")).map_err(|e|e.to_string())?;
    let _lock = fs::lock::LockedFile::exclusive_if_free(&root.join(".codex/.mustard-install.lock")).map_err(|e|e.to_string())?.ok_or("codex-install-busy-retry")?;
    let config = crate::domain::config::ProjectConfig::load(&root);
    if config.unreadable { return Err("codex-install-mustard-config-unreadable".into()); }
    let config_path = root.join(".codex/config.toml");
    let mut text = match std::fs::read_to_string(&config_path) {
        Ok(text) => text, Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(), Err(e) => return Err(e.to_string()),
    };
    let parsed: toml::Value = toml::from_str(&text).map_err(|_| "codex-install-invalid-toml")?;
    let server = json!({"command":"mustard-rt","args":["mcp","--root",root.to_string_lossy()]});
    if let Some(existing) = parsed.get("mcp_servers").and_then(|value|value.get("mustard")) {
        if serde_json::to_value(existing).map_err(|e|e.to_string())? != server { return Err("codex-install-manual-mcp-preserved".into()); }
    } else {
        text.push_str("\n[mcp_servers.mustard]\n");
        text.push_str(&toml::to_string(&server).map_err(|e|e.to_string())?);
        let _: toml::Value = toml::from_str(&text).map_err(|_| "codex-install-config-conflict")?;
    }
    // Hook layers merge rather than replace. Refuse to plant a second Mustard
    // definition alongside one already configured inline.
    if parsed.get("hooks").is_some_and(|hooks| hooks.to_string().contains("mustard-rt")) {
        return Err("codex-install-inline-mustard-hooks-preserved; use one hook representation per layer".into());
    }
    let hook_path = root.join(".codex/hooks.json");
    let mut hooks = read_object(&hook_path)?;
    if hooks.get("hooks").is_none() { hooks["hooks"] = json!({}); }
    if !hooks["hooks"].is_object() { return Err("codex-install-invalid-hook-map".into()); }
    for event in ["SessionStart", "PreToolUse", "PostToolUse", "UserPromptSubmit", "SubagentStart", "SubagentStop", "Stop", "SessionEnd"] {
        let command = format!("mustard-rt on {event} --host codex");
        if hooks["hooks"].get(event).is_none() { hooks["hooks"][event] = json!([]); }
        let entries = hooks["hooks"][event].as_array_mut().ok_or("codex-install-invalid-hook-list")?;
        let already = entries.iter().any(|entry| {
            let same = entry["hooks"].as_array().is_some_and(|list| list.iter().any(|hook| hook["command"] == command));
            same && (event == "SessionStart" || entry["matcher"].as_str().is_none_or(|matcher| matcher == "*"))
        });
        if !already && entries.iter().any(|entry| entry["hooks"].as_array().is_some_and(|list| list.iter().any(|hook| hook["command"] == command))) {
            return Err("codex-install-narrow-mustard-hook-preserved; review its matcher before reinstalling".into());
        }
        if !already { entries.push(json!({"matcher":"*","hooks":[{"type":"command","command":command,"timeout":30}]})); }
    }
    let mut assets = vec![(".agents/skills/mustard/SKILL.md".to_string(),
        "---\nname: mustard\ndescription: Use Mustard for project code searches, specification and wave execution in projects with mustard.json.\n---\n\nSearch through the mustard MCP search tool, preserving native tool/input and adding a short intent/purpose. CLI fallback: `mustard-rt run search --shell-output --intent \"<question>\" --purpose implement -- rg -n \"<pattern>\" .`. Read a known range with the same search contract's Read operation. Reuse current knowledge before more reading. `mustard-rt run resume` gives the active spec's next step. `mustard-rt run expertise --sync` refreshes navigation profiles without inference. For a missing interpretation, `mustard-rt run knowledge --query \"<question>\" --prepare-session codex` prepares bounded evidence for the current session's native subagent; prefer gpt-6-luna when available, inherit otherwise. Fixed full-input budget: 100000 tokens; isolated task, no parent transcript. No new Codex/Claude CLI or generation API. Complete with the returned receipt contract; unknown model/usage stays unknown. Mandatory rules and wave gates always apply.\n".to_string())];
    for (name, raw) in crate::platform::seeds::agent_texts(config.language().text_or_default()) {
        let body = raw.strip_prefix("---\n").and_then(|s|s.split_once("\n---")).map(|(_,body)|body.trim()).ok_or("codex-install-invalid-agent-template")?;
        assets.push((format!(".codex/agents/mustard-{name}.toml"), toml::to_string(&json!({"name":format!("mustard-{name}"),"description":format!("Mustard {name} executor; obey the binary request and project rules."),"developer_instructions":body.replace("Claude Code","Codex")})).map_err(|e|e.to_string())?));
    }
    let ledger_path = root.join(".codex/.mustard-install.json");
    let ledger = read_object(&ledger_path)?;
    for (path, _) in &assets {
        let absolute = root.join(path);
        if absolute.symlink_metadata().is_ok() {
            if !absolute.canonicalize().map_err(|e|e.to_string())?.starts_with(&root) { return Err("codex-install-asset-outside-project".into()); }
            let bytes = std::fs::read(&absolute).map_err(|e|e.to_string())?;
            if ledger[path] != hash(&bytes) { return Err(format!("codex-install-manual-file-preserved:{path}")); }
        }
    }
    // Make the owned footprint private without hiding other Codex files.
    let mut private: Vec<_> = assets.iter().map(|(path,_)| format!("/{path}")).collect();
    private.extend(["/.codex/.mustard-install.json".into(), "/.codex/.mustard-install.lock".into(), "/mustard.json".into()]);
    let exclusion = crate::platform::git_exclude::ensure_excluded(&root, &private);
    if exclusion.unavailable.is_some_and(|reason| reason.is_blocking()) { return Err("codex-install-private-footprint-failed".into()); }
    let mut changed = Vec::new();
    let mut hashes = BTreeMap::new();
    for (path, content) in assets.iter().chain([(String::from(".codex/config.toml"), text), (String::from(".codex/hooks.json"), serde_json::to_string_pretty(&hooks).map_err(|e|e.to_string())?)].iter()) {
        let absolute = root.join(path);
        if std::fs::read(&absolute).ok().as_deref() != Some(content.as_bytes()) {
            if let Some(parent) = absolute.parent() { fs::create_dir_all(parent).map_err(|e|e.to_string())?; }
            fs::write_atomic(&absolute, content.as_bytes()).map_err(|e|e.to_string())?;
            changed.push(path.clone());
        }
        if assets.iter().any(|(asset,_)| asset == path) { hashes.insert(path.clone(), hash(content.as_bytes())); }
    }
    fs::write_atomic(&ledger_path, &serde_json::to_vec_pretty(&hashes).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    if !crate::domain::config::ProjectConfig::exists(&root) { config.write(&root).map_err(|e|e.to_string())?; }
    Ok(json!({"ok":true,"host":"codex","changed":changed,"configured":true,"active":"unknown",
        "hook_trust":"Review the exact new/changed definitions with /hooks in Codex. Installation never grants trust.",
        "limitations":["Host usage/approval telemetry is not claimed from Claude transcripts.","Mods UI is Claude-specific; use the native panel command in Codex."],"generation_model_calls":0}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn narrow_manual_hook_is_refused_before_any_host_asset_is_written() {
        let dir=tempfile::tempdir().unwrap(); let root=dir.path();
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        let hooks=r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"command":"mustard-rt on PreToolUse --host codex"}]}]}}"#;
        std::fs::write(root.join(".codex/hooks.json"),hooks).unwrap();
        assert!(install(root).unwrap_err().contains("narrow-mustard-hook"));
        assert_eq!(std::fs::read_to_string(root.join(".codex/hooks.json")).unwrap(),hooks);
        assert!(!root.join(".codex/config.toml").exists());
    }
    #[cfg(unix)]
    #[test]
    fn linked_agent_directory_cannot_write_outside_the_project() {
        let dir=tempfile::tempdir().unwrap(); let outside=tempfile::tempdir().unwrap(); let root=dir.path();
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        std::os::unix::fs::symlink(outside.path(),root.join(".codex/agents")).unwrap();
        assert!(install(root).unwrap_err().contains("outside-project"));
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(),0);
        assert!(!root.join(".codex/config.toml").exists());
    }
    #[test]
    fn installation_merges_once_and_preserves_manual_files_and_toml_comments() {
        let dir = tempfile::tempdir().unwrap(); let root = dir.path();
        assert!(crate::platform::git::run(root, &["init"]).ok);
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        std::fs::write(root.join(".codex/config.toml"), "# keep this comment\nmodel = 'existing-model'\n").unwrap();
        std::fs::write(root.join(".codex/hooks.json"), r#"{"other":17,"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"other"}]}]}}"#).unwrap();
        assert_eq!(install(root).unwrap()["active"], "unknown");
        assert_eq!(install(root).unwrap()["changed"], json!([]));
        let text = std::fs::read_to_string(root.join(".codex/config.toml")).unwrap(); assert!(text.starts_with("# keep this comment"));
        let hooks: Value = serde_json::from_str(&std::fs::read_to_string(root.join(".codex/hooks.json")).unwrap()).unwrap();
        assert_eq!(hooks["other"], 17); assert_eq!(hooks["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
        assert!(!root.join(".claude/settings.local.json").exists());
        std::fs::write(root.join(".agents/skills/mustard/SKILL.md"), "manual changed skill").unwrap();
        assert!(install(root).unwrap_err().contains("manual-file-preserved"));
        assert_eq!(std::fs::read_to_string(root.join(".agents/skills/mustard/SKILL.md")).unwrap(), "manual changed skill");
    }
}
