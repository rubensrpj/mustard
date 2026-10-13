//! Exercise the shipped CLI contract, not a second implementation of the UI.
#![allow(clippy::unwrap_used,clippy::expect_used)] // Synthetic fixtures, never runtime paths.
use serde_json::{Value,json};
use std::{io::Write,path::Path,process::{Command,Stdio}};
fn call(root:&Path,args:&[&str],input:Option<&Value>)->Value {
    let mut command=Command::new(env!("CARGO_BIN_EXE_mustard-rt"));command.args(["run"]).args(args).arg("--root").arg(root);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    if input.is_some(){command.stdin(Stdio::piped());}
    let mut child=command.spawn().unwrap();
    if let Some(value)=input {child.stdin.take().unwrap().write_all(value.to_string().as_bytes()).unwrap();}
    let output=child.wait_with_output().unwrap();
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn native_panel_views_profiles_ideas_and_receipts_work_without_ai_or_publication() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::write(root.join("mustard.json"),"{\"rtk\":false,\"ai\":{\"fallback\":false,\"vectors\":false}}").unwrap();
    let initial=call(root,&["panel","--section","project"],None);assert_eq!(initial["ok"],true);assert!(!root.join(".claude").exists());
    let rev=initial["revision"].as_str().unwrap();
    assert_eq!(call(root,&["panel","--section","project","--if-revision",rev],None)["unchanged"],true);
    let idea=call(root,&["ideas","--json",&json!({"schema_version":1,"revision":0,"action":"add","title":"Novo download","detail":"Preservar colunas."}).to_string()],None);
    assert_eq!(idea["id"],"P-1");
    let pane=call(root,&["panel","--section","ideas"],None);assert_eq!(pane["ideas"]["items"][0]["idea"],true);
    let profile=call(root,&["panel-preferences","--json",&json!({"schema_version":1,"revision":0,"selected":"planning"}).to_string()],None);
    assert_eq!(profile["ok"],true);assert_eq!(profile["selected"],"planning");
    let receipt=json!({"kind":"step","agent_id":"test-agent","turn_id":"turn-1","index":0,"model":"fixture","effort":"medium",
        "usage":{"input_tokens":100,"output_tokens":5,"cache_read_input_tokens":20,"cache_creation_input_tokens":0},
        "context":{"tokens":120,"window":200_000,"private":"PRIVATE-BODY-MUST-NOT-BE-STORED"}});
    for _ in 0..2 {assert_eq!(call(root,&["usage-record","--session","session-1"],Some(&receipt))["ok"],true);}
    let usage=call(root,&["panel","--section","usage","--session","session-1"],None);
    let agent=usage["live_usage"]["agents"].as_object().unwrap().values().next().unwrap();
    assert_eq!(agent["requests"],1);assert_eq!(agent["input_tokens"],100);
    assert!(!usage.to_string().contains("PRIVATE-BODY-MUST-NOT-BE-STORED"));
    assert!(!root.join(".claude/judgements").exists());assert!(!root.join(".claude/spec").exists());
    assert!(!root.join(".claude/mustard/publications").exists());
}
#[test]
fn impact_and_diagnostic_export_are_local_and_never_run_the_supplied_operation() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
    std::fs::write(root.join("keep.txt"),"keep").unwrap();
    let preview=call(root,&["impact","--json",&json!({"action":"shell","command":"rm keep.txt"}).to_string()],None);
    assert_eq!(preview["executed"],false);assert_eq!(std::fs::read_to_string(root.join("keep.txt")).unwrap(),"keep");
    let input=json!({"component":"jev","operation":"search","reason":"provider-unavailable","secret":"PRIVATE-UNKNOWN-FIELD"});
    assert_eq!(call(root,&["panel-diagnostics","--record"],Some(&input))["ok"],true);
    let exported=call(root,&["panel-diagnostics","--export"],None);assert_eq!(exported["published"],false);
    let content=std::fs::read_to_string(exported["path"].as_str().unwrap()).unwrap();
    assert!(!content.contains("PRIVATE-UNKNOWN-FIELD"));assert!(content.contains("provider-unavailable"));
}
