//! Exercise the actual Mods/native boundary with synthetic credentials.
#![allow(clippy::unwrap_used)] // Disposable fixtures and process output must be valid in this regression.
use std::process::Command;
use serde_json::{Value,json};

#[test]
fn native_settings_are_safe_versioned_and_change_only_selected_fields() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::create_dir_all(root.join(".claude/agents/mustard")).unwrap();
    let secret="synthetic-cli-settings-key";
    std::fs::write(root.join("mustard.json"),json!({"ai":{"fallback":false},"jev":{"key":secret},"future":"preserve"}).to_string()).unwrap();
    std::fs::write(root.join(".claude/settings.local.json"),"{}").unwrap();
    let run=|patch:Option<Value>| {
        let mut command=Command::new(env!("CARGO_BIN_EXE_mustard-rt"));
        command.args(["run","settings","--root"]).arg(root).current_dir(root).env("MUSTARD_RT_DELEGATED","1");
        if let Some(patch)=patch {command.args(["--json",&patch.to_string()]);}
        let output=command.output().unwrap();assert!(output.status.success());
        let text=String::from_utf8(output.stdout).unwrap();assert!(!text.contains(secret));
        serde_json::from_str::<Value>(&text).unwrap()
    };
    let initial=run(None);assert_eq!(initial["schema_version"],1);
    let patch=json!({"schema_version":1,"revision":initial["revision"],"changes":{"rtk":true,"jev":true,"jev_search":true,"agent_model":"opus","agent_effort":"high"}});
    let result=run(Some(patch.clone()));assert_eq!(result["ok"],true,"{result}");assert_eq!(result["saved"],true);
    assert_eq!(run(Some(patch))["reason"],"settings-conflict");
    let config:Value=serde_json::from_str(&std::fs::read_to_string(root.join("mustard.json")).unwrap()).unwrap();
    assert_eq!(config["future"],"preserve");assert_eq!(config["jev"]["key"],secret);
    assert_eq!(config["ai"]["fallback"],true);assert_eq!(config["judgement"]["search"]["filter"],"jev");
    assert!(std::fs::read_to_string(root.join(".claude/settings.local.json")).unwrap().contains("rtk hook claude"));
    assert!(std::fs::read_to_string(root.join(".claude/agents/mustard/wave.md")).unwrap().contains("model: opus"));
    assert!(!root.join(".claude/judgements").exists());assert!(!root.join(".claude/grain.db").exists());
}
