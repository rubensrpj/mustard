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

#[test]
fn observation_cli_pages_only_the_requested_dispatch_without_loading_a_project_panel() {
    use mustard_core::io::{observations::{self,Receipt},spec_events};
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::write(root.join("mustard.json"),r#"{"ai":{"fallback":false}}"#).unwrap();
    let path=spec_events::spec_file(root,"demo").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // A persisted historical send is sufficient for this narrow read; it
    // must not need an index, transcript, Git checkout or project-wide panel.
    std::fs::write(path,json!({"id":7,"type":"send","at":"2026-01-01T00:00:00Z","wave":2,"text":"Old dispatch"}).to_string()+"\n").unwrap();
    for n in 0..33 {
        observations::record(root,&Receipt{version:1,spec:"demo".into(),dispatch:7,wave:2,host:"claude".into(),
            actor:"fixture".into(),call:format!("call-{n}"),tool:"Bash".into(),source:"git --version".into(),
            input_sha256:observations::digest(b"command"),output_sha256:observations::digest(b"output"),
            exit_code:Some(0),result_status:"returned".into(),observed_at:"2026-01-02T00:00:00Z".into()}).unwrap();
    }
    let run=|cursor:Option<&str>| {
        let mut command=Command::new(env!("CARGO_BIN_EXE_mustard-rt"));
        command.args(["run","panel","--root"]).arg(root).args(["--spec","demo","--wave","2","--observations"])
            .current_dir(root).env("MUSTARD_RT_DELEGATED","1");
        if let Some(cursor)=cursor {command.args(["--observations-before",cursor]);}
        let output=command.output().unwrap();assert!(output.status.success());
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let first=run(None);assert_eq!(first["ok"],true,"{first}");
    assert_eq!(first["receipts"].as_array().unwrap().len(),32);assert!(first.get("project").is_none());
    let last=run(first["next_before"].as_str());assert_eq!(last["receipts"].as_array().unwrap().len(),1);
    assert!(last["next_before"].is_null());assert_eq!(last["wave"],2);
}
