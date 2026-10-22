//! Exercise the shipped CLI contract, not a second implementation of the UI.
#![allow(clippy::unwrap_used,clippy::expect_used)] // Synthetic fixtures, never runtime paths.
#[path = "support/binary.rs"]
mod binary;

use serde_json::{Value,json};
use std::{io::Write,path::Path,process::Stdio};
fn call(root:&Path,args:&[&str],input:Option<&Value>)->Value {
    let mut command=binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"));command.args(["run"]).args(args).arg("--root").arg(root);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    if input.is_some(){command.stdin(Stdio::piped());}
    let mut child=command.spawn().unwrap();
    if let Some(value)=input {child.stdin.take().unwrap().write_all(value.to_string().as_bytes()).unwrap();}
    let output=child.wait_with_output().unwrap();
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn native_panel_screens_and_receipts_work_without_ai_or_publication() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::write(root.join("mustard.json"),"{\"rtk\":false,\"ai\":{\"fallback\":false,\"vectors\":false}}").unwrap();
    let initial=call(root,&["panel","--section","overview"],None);assert_eq!(initial["ok"],true);assert!(!root.join(".claude").exists());
    let rev=initial["revision"].as_str().unwrap();
    assert_eq!(call(root,&["panel","--section","overview","--if-revision",rev],None)["unchanged"],true);
    let receipt=json!({"kind":"step","agent_id":"test-agent","turn_id":"turn-1","index":0,"model":"fixture","effort":"medium",
        "usage":{"input_tokens":100,"output_tokens":5,"cache_read_input_tokens":20,"cache_creation_input_tokens":0},
        "context":{"tokens":120,"window":200_000,"private":"PRIVATE-BODY-MUST-NOT-BE-STORED"}});
    for _ in 0..2 {assert_eq!(call(root,&["usage-record","--session","session-1"],Some(&receipt))["ok"],true);}
    let usage=call(root,&["panel","--section","usage","--session","session-1"],None);
    assert_eq!(usage["session"]["agents"],1,"{usage}");assert_eq!(usage["session"]["working"],1);
    assert!(!usage.to_string().contains("PRIVATE-BODY-MUST-NOT-BE-STORED"));
    assert!(!root.join(".claude/judgements").exists());assert!(!root.join(".claude/spec").exists());
    assert!(!root.join(".claude/mustard/publications").exists());
}
/// The spec asked by name is the one shown. An old spec, of before this
/// version of Mustard, is refused with its reason, on one screen and on the
/// whole panel, and no other spec takes its place; without a name, the panel
/// keeps the choice it always made.
#[test]
fn a_legacy_spec_asked_by_name_is_refused_instead_of_replaced() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::write(root.join("mustard.json"),"{\"rtk\":false,\"ai\":{\"fallback\":false,\"vectors\":false}}").unwrap();
    let open=|name:&str| {
        let path=mustard_core::io::spec_events::spec_file(root,name).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let state=json!({"author":"binary","phase":"plan","branch":format!("feature/{name}"),"base":"dev"});
        mustard_core::io::spec_events::write(&path,"state",state.as_object().cloned().unwrap(),&[]).unwrap();
    };
    open("old");
    mustard_core::io::legacy_specs::mark_once(root,"0.2.24").unwrap();
    open("new");
    for args in [&["panel","--spec","old","--section","spec"][..],&["panel","--spec","old"]] {
        let refused=call(root,args,None);
        assert_eq!(refused["ok"],false,"{args:?}: {refused}");
        assert_eq!(refused["reason"],"legacy-spec","{args:?}: {refused}");
        assert!(refused["hint"].as_str().unwrap_or_default().contains("old"),"{args:?}: {refused}");
        assert!(refused.get("selected_spec").is_none(),"{args:?}: {refused}");
    }
    let asked=call(root,&["panel","--spec","new","--section","spec"],None);
    assert_eq!(asked["selected_spec"],"new","{asked}");assert_eq!(asked["spec"]["name"],"new","{asked}");
    assert_eq!(call(root,&["panel","--spec","new"],None)["selected_spec"],"new");
    let unnamed=call(root,&["panel","--section","spec"],None);
    assert_eq!(unnamed["selected_spec"],"new","{unnamed}");
    let whole=call(root,&["panel"],None);
    assert_eq!(whole["ok"],true,"{whole}");
    let listed:Vec<&Value>=whole["specs"].as_array().unwrap().iter().map(|spec|&spec["name"]).collect();
    assert_eq!(listed,[&json!("new")],"{whole}");
}
#[test]
fn a_diagnostic_record_keeps_only_known_fields_and_stays_local() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
    let input=json!({"component":"jev","operation":"search","reason":"provider-unavailable","secret":"PRIVATE-UNKNOWN-FIELD"});
    assert_eq!(call(root,&["panel-diagnostics","--record"],Some(&input))["ok"],true);
    let content=std::fs::read_to_string(root.join(".claude/mustard/panel/diagnostics.json")).unwrap();
    assert!(!content.contains("PRIVATE-UNKNOWN-FIELD"));assert!(content.contains("provider-unavailable"));
    assert_eq!(call(root,&["panel-diagnostics"],None)["reason"],"diagnostic-needs-record");
}
