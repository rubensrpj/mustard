//! Exercise the actual Mods/native boundary with synthetic credentials.
#![allow(clippy::unwrap_used)] // Disposable fixtures and process output must be valid in this regression.
#[path = "support/binary.rs"]
mod binary;

use serde_json::{Value,json};

#[test]
fn native_settings_are_safe_versioned_and_change_only_selected_fields() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::create_dir_all(root.join(".claude/agents/mustard")).unwrap();
    let secret="synthetic-cli-settings-key";
    std::fs::write(root.join("mustard.json"),json!({"ai":{"fallback":false},"jev":{"key":secret},"future":"preserve"}).to_string()).unwrap();
    std::fs::write(root.join(".claude/settings.local.json"),r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"rtk hook claude"}]}]}}"#).unwrap();
    let run=|patch:Option<Value>| {
        let mut command=binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"));
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
    // The rtk option turns Mustard's own swap; rtk's own hook leaves the local settings.
    assert!(!std::fs::read_to_string(root.join(".claude/settings.local.json")).unwrap().contains("rtk hook claude"));
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
        let mut command=binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"));
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

/// A page of receipts carries, for each one, only what lets the reader pick
/// it: the receipt, its time, its result and one short line of the command.
/// The whole recorded command comes from the receipt, and only for a receipt
/// of the wave asked for.
#[test]
fn the_receipt_page_shows_one_short_command_line_and_the_receipt_returns_the_whole_command() {
    use mustard_core::io::{observations::{self,Receipt},spec_events};
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::write(root.join("mustard.json"),r#"{"ai":{"fallback":false}}"#).unwrap();
    let path=spec_events::spec_file(root,"demo").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path,[json!({"id":7,"type":"send","at":"2026-01-01T00:00:00Z","wave":2,"text":"Old dispatch"}),
        json!({"id":9,"type":"send","at":"2026-01-03T00:00:00Z","wave":3,"text":"Other dispatch"})]
        .map(|event|event.to_string()+"\n").concat()).unwrap();
    // The host keeps up to 300 characters of the command: here a C# test run
    // with a long first line, then a TypeScript check on the second line,
    // grouped by the hook inside the wave's directory.
    let long=format!("cd -- '/work/billing app' && (\ndotnet test tests/Billing.Tests/Billing.Tests.csproj --filter \"FullyQualifiedName~Invoice\" {}\nnpx tsc --noEmit -p web/tsconfig.json",
        "--logger trx ".repeat(20)).chars().take(300).collect::<String>();
    let short="npx tsc --noEmit -p web/tsconfig.json";
    let record=|call:&str,dispatch:u64,source:&str,status:&str,code:i64| observations::record(root,&Receipt{version:1,spec:"demo".into(),dispatch,wave:2,
        host:"claude".into(),actor:"fixture".into(),call:call.into(),tool:"Bash".into(),source:source.into(),
        input_sha256:observations::digest(source.as_bytes()),output_sha256:observations::digest(b"output"),
        exit_code:Some(code),result_status:status.into(),observed_at:"2026-01-02T00:00:00Z".into()}).unwrap().0;
    let mut ids=Vec::new();
    for n in 0..31 {ids.push(record(&format!("call-{n}"),7,&long,"failed",1));}
    let whole=record("short-call",7,short,"returned",0);
    let foreign=record("other-wave",9,&long,"returned",0);
    let run=|extra:&[&str]| {
        let output=binary::isolated(env!("CARGO_BIN_EXE_mustard-rt")).args(["run","panel","--root"]).arg(root)
            .args(["--spec","demo","--wave","2","--observations"]).args(extra)
            .current_dir(root).env("MUSTARD_RT_DELEGATED","1").output().unwrap();
        assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
        (output.stdout.len(),serde_json::from_slice::<Value>(&output.stdout).unwrap())
    };
    let (bytes,page)=run(&[]);
    assert_eq!(page["ok"],true,"{page}");
    let rows=page["receipts"].as_array().unwrap();assert_eq!(rows.len(),32);
    // 32 receipts of 300-character commands used to take about 22 thousand
    // bytes; one short line each keeps the whole page under 9 thousand.
    assert!(bytes<9_000,"page of {bytes} bytes");
    for row in rows {
        let mut keys:Vec<_>=row.as_object().unwrap().keys().map(String::as_str).collect();keys.sort_unstable();
        assert_eq!(keys,["exit_code","observed_at","receipt","result_status","source","tool"],"{row}");
    }
    let cut=rows.iter().find(|row|row["receipt"]!=json!(whole)).unwrap()["source"].as_str().unwrap();
    assert!(!cut.contains('\n') && cut.ends_with('…') && cut.chars().count()==81,"{cut}");
    assert!(cut.starts_with("dotnet test ") && long.contains(cut.trim_end_matches('…')),"{cut}");
    let shown=rows.iter().find(|row|row["receipt"]==json!(whole)).unwrap();
    assert_eq!(shown["source"],short);assert_eq!(shown["result_status"],"returned");assert_eq!(shown["exit_code"],0);
    let (_,detail)=run(&["--receipt",&ids[0]]);
    assert_eq!(detail["ok"],true,"{detail}");
    assert_eq!(detail["source"],json!(long));assert_eq!(detail["receipt"],json!(ids[0]));
    assert_eq!(detail["result_status"],"failed");assert_eq!(detail["exit_code"],1);
    assert_eq!(detail["input_sha256"],json!(observations::digest(long.as_bytes())));
    let (_,refused)=run(&["--receipt",&foreign]);
    assert_eq!(refused["ok"],false,"{refused}");
    assert_eq!(refused["reason"],"observation-belongs-to-another-dispatch");
}

/// A hook puts the session and the agent ahead of a native command, and
/// another wraps it in the wave's directory. The page skips both and shows the
/// start of what the agent ran; an `export` the agent wrote itself stays.
#[test]
fn the_receipt_page_skips_the_identity_a_hook_puts_ahead_of_the_command() {
    use mustard_core::io::{observations::{self,Receipt},spec_events};
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::write(root.join("mustard.json"),r#"{"ai":{"fallback":false}}"#).unwrap();
    let path=spec_events::spec_file(root,"demo").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path,json!({"id":7,"type":"send","at":"2026-01-01T00:00:00Z","wave":2,"text":"Dispatch"}).to_string()+"\n").unwrap();
    let copy="/home/dev/.cache/mustard/copias/shop-1a2b3c4d/invoice-fix/c";
    let search=r#"mustard-rt run search --shell-output --intent "where installments are split" --purpose implement -- rg -n "splitInstallments" web/src"#;
    // As the host records it: up to 300 characters of the command the hooks
    // rewrote, so the prefix leaves only the start of the agent's command.
    let prefixed=format!("cd -- '{copy}' && (\nexport MUSTARD_SESSION_ID='08940799-3a4d-4a42-8fa3-4359f788e06b'\nexport MUSTARD_AGENT_ID='ac769b59cbc71ccc5'\ncd {copy} && {search}\n)")
        .chars().take(300).collect::<String>();
    let own="export NODE_ENV=test\nnpx vitest run web/src/installments.test.ts";
    let record=|call:&str,source:&str| observations::record(root,&Receipt{version:1,spec:"demo".into(),dispatch:7,wave:2,
        host:"claude".into(),actor:"fixture".into(),call:call.into(),tool:"Bash".into(),source:source.into(),
        input_sha256:observations::digest(source.as_bytes()),output_sha256:observations::digest(b"output"),
        exit_code:Some(0),result_status:"returned".into(),observed_at:"2026-01-02T00:00:00Z".into()}).unwrap().0;
    let (agent,written)=(record("prefixed",&prefixed),record("own-export",own));
    let output=binary::isolated(env!("CARGO_BIN_EXE_mustard-rt")).args(["run","panel","--root"]).arg(root)
        .args(["--spec","demo","--wave","2","--observations"]).current_dir(root).env("MUSTARD_RT_DELEGATED","1").output().unwrap();
    let page:Value=serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(page["ok"],true,"{page}");
    let shown=|id:&str| page["receipts"].as_array().unwrap().iter().find(|row|row["receipt"]==json!(id)).unwrap()["source"].as_str().unwrap().to_string();
    let line=shown(&agent);
    assert!(line.starts_with("mustard-rt run search --shell-output ") && line.ends_with('…'),"{line}");
    assert!(search.starts_with(line.trim_end_matches('…')),"{line}");
    assert_eq!(shown(&written),"export NODE_ENV=test…");
}

/// The publication token reaches the binary only on standard input: the
/// same patch as an argument, where another process could read it, is
/// refused without touching the file.
#[test]
fn the_token_patch_comes_on_standard_input_and_never_as_an_argument() {
    use std::io::Write;
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::write(root.join("mustard.json"),r#"{"ai":{"fallback":false}}"#).unwrap();
    let token="synthetic-cloudflare-token-stdin";
    let command=||{let mut command=binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"));
        command.args(["run","settings","--root"]).arg(root).current_dir(root).env("MUSTARD_RT_DELEGATED","1");command};
    let read=|output:std::process::Output|{assert!(output.status.success());let text=String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains(token));serde_json::from_str::<Value>(&text).unwrap()};
    let initial=read(command().output().unwrap());
    let patch=json!({"schema_version":1,"revision":initial["revision"],"changes":{"cloudflare_token":token}}).to_string();
    let before=std::fs::read_to_string(root.join("mustard.json")).unwrap();
    let refused=read(command().args(["--json",&patch]).output().unwrap());
    assert_eq!(refused["reason"],"settings-token-argument","{refused}");
    assert_eq!(std::fs::read_to_string(root.join("mustard.json")).unwrap(),before);
    let mut child=command().arg("--stdin").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(patch.as_bytes()).unwrap();
    let saved=read(child.wait_with_output().unwrap());
    assert_eq!(saved["saved"],true,"{saved}");assert_eq!(saved["publication"]["token_saved"],true,"{saved}");
    assert!(std::fs::read_to_string(root.join("mustard.json")).unwrap().contains(token));
}
