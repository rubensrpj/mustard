//! Verify the installed project, rather than treating files written as success.
//! No remote model, app build, tests, publication or host restart at this door.
//! O único pedido ao serviço é a lista de modelos do Jev com a chave do
//! projeto: não leva texto, e a resposta diz se o serviço aceita a chave.
use std::{path::Path,time::Duration};
use mustard_core::ProjectConfig;
use mustard_core::platform::code_tools::{Budget,ToolRunner,RunOutcome};
use serde_json::{Value,json};
use crate::shared::jev::KeyCheck;
use super::Report;

/// A maior espera pela lista de modelos do serviço.
const KEY_CHECK_WAIT:Duration=Duration::from_secs(10);

pub(super) fn finish(root:&Path,report:&Report,runner:&impl ToolRunner,left:Duration)->Value {
    finish_with_install(root,report,runner,left,mustard_core::platform::harness::installed_plugin(),&crate::shared::jev::service_url())
}

fn finish_with_install(root:&Path,report:&Report,runner:&impl ToolRunner,left:Duration,plugin:Option<mustard_core::platform::harness::InstalledPlugin>,jev_url:&str)->Value {
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
        check("rtk-hook",!switches.rtk_hook_left(),"mustard-rt run upsert"),
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
    // A chave presente é conferida uma vez, dentro do prazo da instalação: a
    // recusa deixa a configuração não pronta, e a falta de resposta a deixa
    // sem conferência, nunca conferida.
    let loaded=key_required.then(||crate::shared::jev::key_in(root,&config,key).ok()).flatten();
    let provider=loaded.as_ref().map(|loaded| {
        budget.within(KEY_CHECK_WAIT,|limit|crate::shared::jev::check_key(&loaded.key,jev_url,limit)).unwrap_or(KeyCheck::Unchecked("timeout"))
    });
    let (key_ready,remedy)=match (&provider,key_required) {
        (_,false) => (true,String::new()),
        (None,true) => (false,"configure an untracked key or TYPESAFE_API_KEY; no provider call was made".to_string()),
        // Sem crédito, a chave vale: o remédio é crédito, não outra chave.
        (Some(KeyCheck::Refused(reason @ "no_credit")),true) => (false,format!("the provider accepted the key but the account has no credit ({reason}); add credit to the account")),
        (Some(KeyCheck::Refused(reason)),true) => (false,format!("the provider refused the key ({reason}); configure a valid untracked key or TYPESAFE_API_KEY")),
        (Some(_),true) => (true,String::new()),
    };
    checks.push(check("jev-configuration",key_ready,&remedy));
    let project_ready=checks.iter().all(|check|check["ready"]==true);
    let host_version=plugin.as_ref().is_some_and(|plugin|plugin_current(plugin,&expected,SHIPPED_HOOKS_DIGEST));
    let host_binaries=host_version && plugin.as_ref().is_some_and(|plugin|host_binaries_ready(plugin,&expected,runner,&budget));
    let host_enabled=host_version && plugin.as_ref().is_some_and(|plugin|enabled_plugin(plugin,runner,&budget));
    let host_ready=host_binaries && host_enabled;
    checks.push(check("plugin-version",host_version,&plugin_version_remedy(root,plugin.as_ref(),&expected)));
    // Os programas e a ativação só se conferem sobre o plugin desta
    // compilação: sem ele, ficam não prontos sem remédio próprio, que não
    // consertaria nada.
    let dependent=|remedy:&'static str| if host_version {remedy} else {NEEDS_PLUGIN_VERSION};
    checks.push(check("plugin-binaries",host_binaries,dependent("run the installed plugin's bin/mustard-boot --version; retry mustard-rt run upsert")));
    checks.push(check("plugin-enabled",host_enabled,dependent("enable Mustard with claude plugin enable <installed-plugin-id>, then restart Claude Code")));
    let mut receipt=json!({"schema_version":1,"version":expected,"at":chrono::Utc::now().to_rfc3339(),
        "complete":project_ready && host_ready,"project_ready":project_ready,"host_ready":host_ready,
        "checks":checks,"scan":{"ok":scan["ok"],"files":scan["files"],"read":scan["read"],"reason":scan["reason"],"error":scan["error"]},
        "vectors_enabled":config.ai_vectors_enabled(),"jev_requested":key_required,"provider_verified":provider==Some(KeyCheck::Verified),
        "restart_required":report.plugin_refresh.restart.is_some()});
    match provider {
        Some(KeyCheck::Refused(reason)) => receipt["provider_refusal"]=json!(reason),
        Some(KeyCheck::Unchecked(reason)) => receipt["provider_unchecked"]=json!(reason),
        _ => {}
    }
    let saved=serde_json::to_vec(&receipt).is_ok_and(|bytes|mustard_core::io::fs::write_atomic(root.join(".claude/mustard/installation.json"),&bytes).is_ok());
    if !saved {receipt["complete"]=json!(false);receipt["receipt_saved"]=json!(false);}
    receipt
}

/// O resumo dos ganchos que esta compilação entrega, gravado pelo `build.rs`.
const SHIPPED_HOOKS_DIGEST:&str=env!("MUSTARD_PLUGIN_HOOKS_DIGEST");

/// O remédio das conferências do plugin que dependem da versão dele.
const NEEDS_PLUGIN_VERSION:&str="not checked: needs plugin-version";

/// O remédio da versão do plugin. O plugin instalado mais novo que o
/// programa que responde não se conserta reinstalando o plugin: quem está
/// atrás é o programa, e o remédio cita as duas versões e manda subir o
/// programa à do plugin — no repositório do Mustard, avançando a branch e
/// recompilando o programa de desenvolvimento. Plugin ausente, mais velho ou
/// com os ganchos trocados sob o mesmo número leva o remédio de reinstalar.
fn plugin_version_remedy(root:&Path,plugin:Option<&mustard_core::platform::harness::InstalledPlugin>,expected:&str)->String {
    match plugin {
        Some(plugin) if mustard_core::platform::harness::is_behind(expected,&plugin.version) => {
            let installed=&plugin.version;
            let update=if mustard_core::mustard_checkout(root).is_some() {
                format!("advance this branch to {installed} and rebuild the development program with cargo build --profile mustard-dev")
            } else {
                format!("update the Mustard program to {installed} by reinstalling the Mustard bundle")
            };
            format!("the Mustard program answering ({expected}) is behind the installed plugin ({installed}); {update}, then retry mustard-rt run upsert")
        }
        _ => "update or reinstall the Mustard plugin so its hooks match this build, then restart Claude Code".into(),
    }
}

/// O plugin instalado é o desta compilação: o registro e o manifesto dizem o
/// número esperado, e os ganchos da cópia que o anfitrião carrega têm o
/// conteúdo destes. O número sozinho não basta: o desenvolvimento repete o
/// número, e o cache do anfitrião guarda os ganchos velhos sob ele.
fn plugin_current(plugin:&mustard_core::platform::harness::InstalledPlugin,expected:&str,hooks:&str)->bool {
    plugin.version==expected
        && std::fs::read_to_string(plugin.dir.join(".claude-plugin/plugin.json")).ok().and_then(|raw|serde_json::from_str::<Value>(&raw).ok()).is_some_and(|doc|doc["version"]==expected)
        && mustard_core::io::hooks_digest::hooks_digest(&plugin.dir).is_some_and(|digest|digest==hooks)
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
    /// A cópia do plugin em `dir` que o anfitrião carregaria: o manifesto com
    /// o número desta compilação e as pastas de ganchos que ela entrega.
    fn shipped_plugin_copy(dir:&Path)->mustard_core::platform::harness::InstalledPlugin {
        let version=mustard_core::harness_version();
        std::fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
        std::fs::write(dir.join(".claude-plugin/plugin.json"),json!({"name":"mustard","version":version}).to_string()).unwrap();
        let shipped=Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin");
        for folder in mustard_core::io::hooks_digest::HOOK_FOLDERS {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
            for entry in std::fs::read_dir(shipped.join(folder)).unwrap() {
                let path=entry.unwrap().path();
                if path.is_file() {std::fs::copy(&path,dir.join(folder).join(path.file_name().unwrap())).unwrap();}
            }
        }
        mustard_core::platform::harness::InstalledPlugin{dir:dir.into(),version}
    }
    /// Um plugin instalado com o número desta compilação só confere quando os
    /// ganchos da cópia e a `ui/` que eles importam têm o conteúdo dos que ela
    /// entrega: o mesmo número com um `register.js` ou um tema velho fica não
    /// pronto, e a cópia que só troca o fim de linha confere.
    #[test]
    fn a_cached_plugin_with_the_same_version_and_old_hooks_is_not_current() {
        let dir=tempfile::tempdir().unwrap();
        let plugin=shipped_plugin_copy(dir.path());
        let version=plugin.version.clone();
        let hooks=dir.path().join("hooks");
        assert!(plugin_current(&plugin,&version,SHIPPED_HOOKS_DIGEST),"the hooks of this build are current");
        let theme=std::fs::read_to_string(dir.path().join("ui/theme.js")).unwrap();
        std::fs::write(dir.path().join("ui/theme.js"),format!("{theme}\n// old theme\n")).unwrap();
        assert!(!plugin_current(&plugin,&version,SHIPPED_HOOKS_DIGEST),"the same version with an old theme the hooks import is not current");
        std::fs::write(dir.path().join("ui/theme.js"),theme).unwrap();
        let register=std::fs::read_to_string(hooks.join("register.js")).unwrap();
        std::fs::write(hooks.join("register.js"),register.replace('\n',"\r\n")).unwrap();
        assert!(plugin_current(&plugin,&version,SHIPPED_HOOKS_DIGEST),"a copy with other line endings holds the same hooks");
        std::fs::write(hooks.join("register.js"),format!("{register}\n// old build\n")).unwrap();
        assert!(!plugin_current(&plugin,&version,SHIPPED_HOOKS_DIGEST),"the same version with old hooks is not the plugin of this build");
        std::fs::write(hooks.join("register.js"),register).unwrap();
        std::fs::remove_file(hooks.join("hooks.json")).unwrap();
        assert!(!plugin_current(&plugin,&version,SHIPPED_HOOKS_DIGEST),"a copy missing the manifest is not current");
    }
    /// A conclusão confere a versão do plugin pelos ganchos da cópia, não só
    /// pelo número: a cópia com o número e os ganchos desta compilação tem a
    /// versão pronta, e a mesma cópia com um gancho velho fica não pronta.
    #[test]
    fn the_installation_marks_the_plugin_version_not_ready_when_only_the_hooks_changed() {
        let project=tempfile::tempdir().unwrap();
        let report=installed_project(project.path());
        let cached=tempfile::tempdir().unwrap();
        let plugin=shipped_plugin_copy(cached.path());
        let plugin_version=|plugin| {
            let receipt=finish_with_install(project.path(),&report,&MissingPrograms,Duration::from_secs(1),Some(plugin),"http://127.0.0.1:9/v1/systemone");
            component(&receipt,"plugin-version")
        };
        let current=plugin_version(plugin.clone());
        assert_eq!(current["ready"],true,"the hooks of this build under its number are the current plugin: {current}");
        let register=cached.path().join("hooks/register.js");
        let body=std::fs::read_to_string(&register).unwrap();
        std::fs::write(&register,format!("{body}\n// old build\n")).unwrap();
        let old=plugin_version(plugin);
        assert_eq!(old["ready"],false,"the same number with an old hook is not the plugin of this build: {old}");
        assert!(old["remedy"].as_str().is_some_and(|remedy|remedy.contains("reinstall the Mustard plugin")),"{old}");
    }
    /// O plugin instalado mais novo que o programa que responde tem a versão
    /// não pronta com o remédio que cita as duas versões e manda subir o
    /// programa; no repositório do Mustard, recompilando o de
    /// desenvolvimento. O plugin mais velho e o ausente seguem com o remédio
    /// de reinstalar o plugin.
    #[test]
    fn a_plugin_newer_than_the_answering_program_names_the_program_behind() {
        let project=tempfile::tempdir().unwrap();
        let report=installed_project(project.path());
        let cached=tempfile::tempdir().unwrap();
        let plugin=shipped_plugin_copy(cached.path());
        let expected=mustard_core::harness_version();
        let newer=format!("{expected}.1");
        let remedy=|plugin:Option<mustard_core::platform::harness::InstalledPlugin>| {
            let receipt=finish_with_install(project.path(),&report,&MissingPrograms,Duration::from_secs(1),plugin,"http://127.0.0.1:9/v1/systemone");
            let version=component(&receipt,"plugin-version");
            assert_eq!(version["ready"],false,"{version}");
            version["remedy"].as_str().unwrap().to_string()
        };
        let ahead=remedy(Some(mustard_core::platform::harness::InstalledPlugin{version:newer.clone(),..plugin.clone()}));
        assert!(ahead.contains("behind the installed plugin") && ahead.contains(&format!("({expected})")) && ahead.contains(&format!("({newer})")),"{ahead}");
        assert!(!ahead.contains("reinstall the Mustard plugin") && !ahead.contains("cargo build"),"{ahead}");
        std::fs::create_dir_all(project.path().join("apps/rt")).unwrap();
        std::fs::write(project.path().join("apps/rt/Cargo.toml"),"[package]\nname = \"mustard-rt\"\n").unwrap();
        let in_mustard=remedy(Some(mustard_core::platform::harness::InstalledPlugin{version:newer.clone(),..plugin.clone()}));
        assert!(in_mustard.contains(&format!("({expected})")) && in_mustard.contains(&format!("advance this branch to {newer}")) && in_mustard.contains("cargo build --profile mustard-dev"),"{in_mustard}");
        let older=remedy(Some(mustard_core::platform::harness::InstalledPlugin{version:"0.0.1".into(),..plugin}));
        assert!(older.contains("reinstall the Mustard plugin") && !older.contains("behind"),"{older}");
        let absent=remedy(None);
        assert!(absent.contains("reinstall the Mustard plugin") && !absent.contains("behind"),"{absent}");
    }
    /// Os programas e a ativação do plugin só se conferem sobre o plugin
    /// desta compilação: com a versão não pronta, os dois ficam não prontos
    /// com o remédio de não conferidos, e o recibo incompleto; com a versão
    /// pronta, cada um segue com o próprio remédio.
    #[test]
    fn the_plugin_checks_that_need_its_version_are_not_checked_without_it() {
        let project=tempfile::tempdir().unwrap();
        let report=installed_project(project.path());
        let cached=tempfile::tempdir().unwrap();
        let plugin=shipped_plugin_copy(cached.path());
        let receipt_for=|plugin| finish_with_install(project.path(),&report,&MissingPrograms,Duration::from_secs(1),Some(plugin),"http://127.0.0.1:9/v1/systemone");
        let current=receipt_for(plugin.clone());
        assert_eq!(component(&current,"plugin-version")["ready"],true,"{current}");
        assert!(component(&current,"plugin-binaries")["remedy"].as_str().is_some_and(|remedy|remedy.contains("mustard-boot")),"{current}");
        assert!(component(&current,"plugin-enabled")["remedy"].as_str().is_some_and(|remedy|remedy.contains("claude plugin enable")),"{current}");
        let newer=format!("{}.1",mustard_core::harness_version());
        let ahead=receipt_for(mustard_core::platform::harness::InstalledPlugin{version:newer,..plugin});
        assert_eq!(ahead["complete"],false,"{ahead}");
        assert_eq!(ahead["host_ready"],false,"{ahead}");
        for name in ["plugin-binaries","plugin-enabled"] {
            let dependent=component(&ahead,name);
            assert_eq!(dependent["ready"],false,"{ahead}");
            assert_eq!(dependent["remedy"],"not checked: needs plugin-version","{ahead}");
        }
    }
    #[test]
    fn files_written_without_a_working_scan_and_host_are_not_a_complete_installation() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();
        let report=Report{scan:Some(json!({"ok":false,"reason":"missing-scan"})),..installed_project(root)};
        let result=finish_with_install(root,&report,&MissingPrograms,Duration::from_secs(1),None,"http://127.0.0.1:9/v1/systemone");
        assert_eq!(result["complete"],false);
        assert_eq!(result["host_ready"],false);
        assert!(result["checks"].as_array().unwrap().iter().any(|check|check["component"]=="scan" && check["ready"]==false));
        assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(root.join(".claude/mustard/installation.json")).unwrap()).unwrap()["complete"],false);
        assert!(!root.join(".claude/grain.db").exists());
    }

    /// Um serviço em 127.0.0.1 que responde `status` à primeira conexão e
    /// devolve a linha do pedido e se ele trouxe corpo.
    fn models_service(status:u16)->(String,std::sync::mpsc::Receiver<(String,bool)>) {
        use std::io::{BufRead,BufReader,Write};
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url=format!("http://{}/v1/systemone",listener.local_addr().unwrap());
        let (tx,rx)=std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let Ok((stream,_))=listener.accept() else {return};
            let mut reader=BufReader::new(&stream);
            let mut first=String::new();
            let _=reader.read_line(&mut first);
            let mut body=false;
            loop {
                let mut line=String::new();
                if reader.read_line(&mut line).unwrap_or(0)==0 || line=="\r\n" {break;}
                let lower=line.to_ascii_lowercase();
                body|=(lower.starts_with("content-length:") && lower.trim_end()!="content-length: 0") || lower.starts_with("transfer-encoding:");
            }
            let mut stream=&stream;
            let _=write!(stream,"HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _=tx.send((first.trim_end().to_string(),body));
        });
        (url,rx)
    }

    /// O projeto instalado em `root`, num repositório git, e o relatório da
    /// instalação dele, sem anfitrião e sem passada do scan.
    fn installed_project(root:&Path)->Report {
        assert!(std::process::Command::new("git").args(["init","-q"]).current_dir(root).status().unwrap().success());
        let project=mustard_core::platform::project_seed::upsert_project(root,Some(&mustard_core::harness_version()),mustard_core::platform::project_seed::InstallMode::Private).unwrap();
        Report{project,plugin_refresh:super::super::skipped(None,"no installed host".into()),code_tool_warnings:vec![],search_warning:None,local_files_found:None,installation:None,scan:None}
    }

    /// O projeto instalado em `root` com a busca pelo Jev e a chave no
    /// `mustard.json`, e o relatório da instalação dele.
    fn keyed_install(root:&Path)->Report {
        let report=installed_project(root);
        std::fs::write(root.join("mustard.json"),json!({"ai":{"fallback":true},"judgement":{"search":{"filter":"jev"}},"jev":{"key":"sk-installed"}}).to_string()).unwrap();
        report
    }

    /// A conferência `name` do recibo da instalação.
    fn component(receipt:&Value,name:&str)->Value {
        receipt["checks"].as_array().unwrap().iter().find(|check|check["component"]==name).cloned().unwrap()
    }

    fn jev(receipt:&Value)->Value {
        component(receipt,"jev-configuration")
    }

    /// Com a chave presente, a instalação lista os modelos do serviço, sem
    /// mandar texto: a chave recusada (401) grava `key_refused` e deixa a
    /// configuração do Jev não pronta; a conta sem crédito (402) também, com
    /// o remédio de pôr crédito, não o de trocar a chave; a aceita grava a
    /// chave como conferida; o erro do serviço (503) e a falta de quem
    /// responda deixam a chave sem conferência, cada um com a sua palavra.
    #[test]
    fn a_present_key_is_checked_on_the_provider_model_list() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();
        let report=keyed_install(root);

        let (url,seen)=models_service(401);
        let refused=finish_with_install(root,&report,&MissingPrograms,Duration::from_secs(5),None,&url);
        let (request,body)=seen.recv_timeout(Duration::from_secs(10)).expect("the installation lists the models with the key");
        assert_eq!(request,"GET /v1/models HTTP/1.1");
        assert!(!body,"the check sends no text");
        assert_eq!(refused["provider_verified"],false,"{refused}");
        assert_eq!(refused["provider_refusal"],"key_refused","{refused}");
        assert_eq!(jev(&refused)["ready"],false,"{refused}");
        let saved:Value=serde_json::from_str(&std::fs::read_to_string(root.join(".claude/mustard/installation.json")).unwrap()).unwrap();
        assert_eq!(saved["provider_refusal"],"key_refused");

        let (url,_seen)=models_service(402);
        let broke=finish_with_install(root,&report,&MissingPrograms,Duration::from_secs(5),None,&url);
        assert_eq!(broke["provider_refusal"],"no_credit","{broke}");
        assert_eq!(jev(&broke)["ready"],false,"{broke}");
        let remedy=jev(&broke)["remedy"].as_str().unwrap_or_default().to_string();
        assert!(remedy.contains("add credit"),"{broke}");
        assert!(!remedy.contains("configure a valid"),"no credit is not a key to change: {broke}");

        let (url,_seen)=models_service(503);
        let failing=finish_with_install(root,&report,&MissingPrograms,Duration::from_secs(5),None,&url);
        assert_eq!(failing["provider_verified"],false,"{failing}");
        assert_eq!(failing["provider_unchecked"],"service_error","{failing}");
        assert_eq!(failing.get("provider_refusal"),None,"{failing}");
        assert_eq!(jev(&failing)["ready"],true,"a failing service does not refuse a present key: {failing}");

        let (url,_seen)=models_service(200);
        let verified=finish_with_install(root,&report,&MissingPrograms,Duration::from_secs(5),None,&url);
        assert_eq!(verified["provider_verified"],true,"{verified}");
        assert_eq!(jev(&verified)["ready"],true,"{verified}");

        let silent=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url=format!("http://{}/v1/systemone",silent.local_addr().unwrap());
        drop(silent);
        let unchecked=finish_with_install(root,&report,&MissingPrograms,Duration::from_secs(5),None,&url);
        assert_eq!(unchecked["provider_verified"],false,"{unchecked}");
        assert_eq!(unchecked["provider_unchecked"],"network","{unchecked}");
        assert_eq!(jev(&unchecked)["ready"],true,"an unanswered check does not refuse a present key: {unchecked}");
    }

    /// O serviço que aceita a conexão e nunca responde não prende a
    /// instalação: com orçamento de sobra, ela volta no prazo da conferência
    /// (10 s), com a chave sem conferência por tempo esgotado.
    #[test]
    fn a_silent_provider_does_not_hold_the_installation_past_the_check_wait() {
        use std::io::Read;
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url=format!("http://{}/v1/systemone",listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let Ok((mut stream,_))=listener.accept() else {return};
            let mut buffer=[0u8;1024];
            while stream.read(&mut buffer).is_ok_and(|read|read>0) {}
        });
        let (tx,rx)=std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let dir=tempfile::tempdir().unwrap();
            let report=keyed_install(dir.path());
            let started=std::time::Instant::now();
            let receipt=finish_with_install(dir.path(),&report,&MissingPrograms,Duration::from_secs(60),None,&url);
            let _=tx.send((started.elapsed(),receipt));
        });
        let (took,receipt)=rx.recv_timeout(KEY_CHECK_WAIT+Duration::from_secs(20)).expect("the installation returns within the check wait");
        assert!(took<KEY_CHECK_WAIT+Duration::from_secs(2),"the check waited {took:?}");
        assert_eq!(receipt["provider_verified"],false,"{receipt}");
        assert_eq!(receipt["provider_unchecked"],"timeout","{receipt}");
        assert_eq!(jev(&receipt)["ready"],true,"{receipt}");
    }
}
