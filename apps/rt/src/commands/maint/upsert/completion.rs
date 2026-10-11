//! Verify the installed project, rather than treating files written as success.
//! No remote model, app build, tests, publication or host restart at this door.
use std::{path::Path,time::Duration};
use mustard_core::ProjectConfig;
use mustard_core::platform::code_tools::{Budget,ToolRunner,RunOutcome};
use serde_json::{Value,json};
use super::Report;

pub(super) fn finish(root:&Path,report:&Report,runner:&impl ToolRunner,left:Duration)->Value {
    finish_with_install(root,report,runner,left,mustard_core::platform::harness::installed_plugin())
}

fn finish_with_install(root:&Path,report:&Report,runner:&impl ToolRunner,left:Duration,plugin:Option<mustard_core::platform::harness::InstalledPlugin>)->Value {
    let budget=Budget::start(runner,left);
    let scan=report.scan.as_ref().unwrap_or(&Value::Null);
    let config=ProjectConfig::load(root);
    let texts=mustard_core::platform::project_seed::harness_texts(config.language().text_or_default(),config.agent_settings());
    let current_texts=texts.iter().all(|(path,text)|mustard_core::io::fs::read_to_string(root.join(".claude").join(path)).is_ok_and(|actual|actual==*text));
    let settings=mustard_core::io::fs::read_to_string(root.join(".claude/settings.local.json")).ok()
        .and_then(|text|serde_json::from_str::<Value>(&text).ok());
    let switches=mustard_core::Switches::read(root);
    let expected=mustard_core::harness_version();
    let mut checks=vec![
        check("project-version",!config.unreadable && config.version.as_deref()==Some(&expected),"mustard-rt run upsert"),
        check("shipped-texts",current_texts,"mustard-rt run upsert"),
        check("local-settings",settings.as_ref().is_some_and(Value::is_object),"mustard-rt run upsert"),
        check("rtk-hook",!switches.rtk_diverges(),"mustard-rt run upsert"),
        check("scan",scan["ok"]==true && root.join(".claude/grain.db").is_file(),"mustard-rt run scan; mustard-rt run upsert"),
        check("spec-search",report.search_warning.is_none(),"mustard-rt run upsert"),
        check("language-tools",report.code_tool_warnings.is_empty(),"see codeToolWarnings"),
    ];
    let parent=std::env::current_exe().ok().and_then(|path|path.parent().map(Path::to_path_buf));
    for name in ["scan","rtk","rg","mustard"] {
        let file=if cfg!(windows){format!("{name}.exe")}else{name.into()};
        let companion=parent.as_ref().map(|parent|parent.join(file)).filter(|path|path.is_file());
        let program=companion.as_deref().and_then(Path::to_str).unwrap_or(name);
        let ready=program_ready(program,matches!(name,"scan"|"mustard").then_some(expected.as_str()),runner,&budget);
        checks.push(check(&format!("program-{name}"),ready,"reinstall the current Mustard bundle; retry mustard-rt run upsert"));
    }
    let key_required=config.ai_fallback_enabled() && ["search","wave-planning","context"].iter()
        .any(|purpose|mustard_core::io::jev_gate::setting_allows(config.judgement_filter(purpose)));
    let key_env=mustard_core::io::jev_gate::KEY_ENV;
    let key=std::env::var(key_env).ok().or_else(|| {
        // Claude applies these private environment settings on session start.
        // Inspect availability without exporting the value or calling Jev.
        crate::commands::flow::round::local_file_ignored(root,".claude/settings.local.json")
            .then(||settings.as_ref()?.get("env")?.get(key_env)?.as_str().map(str::to_string)).flatten()
    });
    let key_ready=!key_required || mustard_core::io::jev_gate::find_key(root,&config,key).is_ok();
    checks.push(check("jev-configuration",key_ready,"configure an untracked key or TYPESAFE_API_KEY; no provider call was made"));
    let project_ready=checks.iter().all(|check|check["ready"]==true);
    let host_version=plugin.as_ref().is_some_and(|plugin|plugin.version==expected &&
        std::fs::read_to_string(plugin.dir.join(".claude-plugin/plugin.json")).ok().and_then(|raw|serde_json::from_str::<Value>(&raw).ok()).is_some_and(|doc|doc["version"]==expected));
    let host_binaries=host_version && plugin.as_ref().is_some_and(|plugin|host_binaries_ready(plugin,&expected,runner,&budget));
    let host_enabled=host_version && plugin.as_ref().is_some_and(|plugin|enabled_plugin(plugin,runner,&budget));
    let host_ready=host_binaries && host_enabled;
    checks.push(check("plugin-version",host_version,"update the Mustard plugin, then restart Claude Code"));
    checks.push(check("plugin-binaries",host_binaries,"run the installed plugin's bin/mustard-boot --version; retry mustard-rt run upsert"));
    checks.push(check("plugin-enabled",host_enabled,"enable Mustard with claude plugin enable <installed-plugin-id>, then restart Claude Code"));
    let mut receipt=json!({"schema_version":1,"version":expected,"at":chrono::Utc::now().to_rfc3339(),
        "complete":project_ready && host_ready,"project_ready":project_ready,"host_ready":host_ready,
        "checks":checks,"scan":{"ok":scan["ok"],"files":scan["files"],"read":scan["read"],"reason":scan["reason"],"error":scan["error"]},
        "vectors_enabled":config.ai_vectors_enabled(),"jev_requested":key_required,"provider_verified":false,
        "restart_required":report.plugin_refresh.restart.is_some()});
    let saved=serde_json::to_vec(&receipt).is_ok_and(|bytes|mustard_core::io::fs::write_atomic(root.join(".claude/mustard/installation.json"),&bytes).is_ok());
    if !saved {receipt["complete"]=json!(false);receipt["receipt_saved"]=json!(false);}
    receipt
}

fn enabled_plugin(plugin:&mustard_core::platform::harness::InstalledPlugin,runner:&impl ToolRunner,budget:&Budget<'_,impl ToolRunner>)->bool {
    let Some((RunOutcome::Succeeded,text))=budget.within(Duration::from_secs(3),|limit|runner.output("claude",&["plugin","list","--json"],limit)) else {return false;};
    let Ok(list)=serde_json::from_str::<Value>(&text) else {return false;};
    list.as_array().is_some_and(|rows|rows.iter().any(|row| {
        row["id"].as_str().is_some_and(|id|id.split('@').next()==Some(mustard_core::PLUGIN_NAME))
            && row["enabled"]==true && row["version"]==plugin.version
            && row["installPath"].as_str().is_some_and(|path|Path::new(path)==plugin.dir)
    }))
}

fn program_ready(program:&str,expected:Option<&str>,runner:&impl ToolRunner,budget:&Budget<'_,impl ToolRunner>)->bool {
    let (status,version)=budget.within(Duration::from_secs(2),|limit|runner.output(program,&["--version"],limit)).unwrap_or((RunOutcome::Failed,String::new()));
    status==RunOutcome::Succeeded && expected.is_none_or(|expected|version.split_whitespace().any(|part|part==expected))
}

fn host_binaries_ready(plugin:&mustard_core::platform::harness::InstalledPlugin,expected:&str,runner:&impl ToolRunner,budget:&Budget<'_,impl ToolRunner>)->bool {
    let bin=plugin.dir.join("bin");
    let name=|name:&str| if cfg!(windows){format!("{name}.exe")}else{name.into()};
    let names=["mustard-rt","scan","mustard","rtk","rg"];
    let stamp=std::fs::read_to_string(bin.join(".version")).ok();
    if names.iter().any(|item|!bin.join(name(item)).is_file()) || stamp.as_ref().is_some_and(|stamp|stamp.trim()!=expected) {
        let boot=bin.join(if cfg!(windows){"mustard-boot.cmd"}else{"mustard-boot"});
        if !boot.is_file() {return false;}
        let Some(boot)=boot.to_str() else {return false;};
        // Use the plugin's existing bootstrap; never manufacture a stamp or
        // overwrite a development build, and never restart a loaded session.
        let _=budget.within(Duration::from_secs(30),|limit|runner.output(boot,&["--version"],limit));
    }
    let stamp_matches=std::fs::read_to_string(bin.join(".version")).map_or(true,|stamp|stamp.trim()==expected);
    stamp_matches && names.iter().all(|item| {
        let path=bin.join(name(item));
        path.is_file() && path.to_str().is_some_and(|path|program_ready(path,(!matches!(*item,"rtk"|"rg")).then_some(expected),runner,budget))
    })
}

fn check(name:&str,ready:bool,remedy:&str)->Value {
    let mut result=json!({"component":name,"ready":ready});
    if !ready {result["remedy"]=json!(remedy);}
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    struct MissingPrograms;
    impl ToolRunner for MissingPrograms {
        fn on_path(&self,_:&str)->bool {false}
        fn run(&self,_:&str,_:&[&str])->bool {panic!("no application command is executed")}
        fn output(&self,_:&str,_:&[&str],_:Duration)->(RunOutcome,String) {(RunOutcome::Failed,String::new())}
        fn found_off_path(&self,_:&str)->Option<std::path::PathBuf> {None}
    }
    struct Versions {stale_scan:bool}
    impl ToolRunner for Versions {
        fn on_path(&self,_:&str)->bool {false}
        fn run(&self,_:&str,_:&[&str])->bool {panic!("no installer or application command is expected")}
        fn output(&self,program:&str,args:&[&str],_:Duration)->(RunOutcome,String) {
            assert_eq!(args,["--version"]);
            assert!(!program.contains("mustard-boot"),"complete files need no bootstrap");
            let stale=self.stale_scan && Path::new(program).file_stem().is_some_and(|name|name=="scan");
            (RunOutcome::Succeeded,format!("program {}",if stale {"0.0.0".into()}else{mustard_core::harness_version()}))
        }
        fn found_off_path(&self,_:&str)->Option<std::path::PathBuf> {None}
    }
    #[test]
    fn a_registry_version_alone_does_not_prove_the_plugin_is_ready() {
        let dir=tempfile::tempdir().unwrap();
        let plugin=mustard_core::platform::harness::InstalledPlugin{dir:dir.path().into(),version:mustard_core::harness_version()};
        let good=Versions{stale_scan:false};
        let budget=Budget::start(&good,Duration::from_secs(1));
        assert!(!host_binaries_ready(&plugin,&plugin.version,&good,&budget));
        std::fs::create_dir_all(dir.path().join("bin")).unwrap();
        for name in ["mustard-rt","scan","mustard","rtk","rg"] {
            let file=if cfg!(windows){format!("{name}.exe")}else{name.into()};
            std::fs::write(dir.path().join("bin").join(file),"fixture").unwrap();
        }
        std::fs::write(dir.path().join("bin/.version"),&plugin.version).unwrap();
        assert!(host_binaries_ready(&plugin,&plugin.version,&good,&budget));
        let stale=Versions{stale_scan:true};
        let budget=Budget::start(&stale,Duration::from_secs(1));
        assert!(!host_binaries_ready(&plugin,&plugin.version,&stale,&budget),"a current stamp cannot hide an old companion");
    }
    #[test]
    fn files_written_without_a_working_scan_and_host_are_not_a_complete_installation() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();
        assert!(std::process::Command::new("git").args(["init","-q"]).current_dir(root).status().unwrap().success());
        let project=mustard_core::platform::project_seed::upsert_project(root,Some(&mustard_core::harness_version()),mustard_core::platform::project_seed::InstallMode::Private).unwrap();
        let report=Report{project,plugin_refresh:super::super::skipped(None,"no installed host".into()),code_tool_warnings:vec![],search_warning:None,local_files_found:None,installation:None,scan:Some(json!({"ok":false,"reason":"missing-scan"}))};
        let result=finish_with_install(root,&report,&MissingPrograms,Duration::from_secs(1),None);
        assert_eq!(result["complete"],false);
        assert_eq!(result["host_ready"],false);
        assert!(result["checks"].as_array().unwrap().iter().any(|check|check["component"]=="scan" && check["ready"]==false));
        assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(root.join(".claude/mustard/installation.json")).unwrap()).unwrap()["complete"],false);
        assert!(!root.join(".claude/grain.db").exists());
    }
}
