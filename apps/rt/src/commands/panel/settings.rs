//! Explicit, native settings edits. Only the allowlisted projection leaves the
//! binary; credentials and unknown project choices never enter a Mods response.
use std::path::Path;
use mustard_core::{ProjectConfig,domain::config::{AGENT_EFFORTS,FilterSetting}};
use serde_json::{Value,json};

const FIELDS:[&str;16]=["enabled","rtk","vectors","jev","jev_search","jev_wave_planning","jev_context","agent_model","agent_effort","search_answer","session_claude_model","session_codex_model",
    "build_command","test_command","lint_command","max_compiling_waves"];
/// Os comandos do projeto, gravados na raiz do `mustard.json` com o nome em
/// camelCase; o nome antigo, em snake_case, é o próprio campo.
const COMMANDS:[(&str,&str);3]=[("build_command","buildCommand"),("test_command","testCommand"),("lint_command","lintCommand")];
const PURPOSES:[(&str,&str);3]=[("jev_search","search"),("jev_wave_planning","wave-planning"),("jev_context","context")];

pub(super) fn run(start:&Path,patch:Option<&str>)->Value {
    let root=mustard_core::io::spec_events::spec_root(start);
    if let Some(patch)=patch {apply(&root,patch)} else {read(&root)}
}

fn refused(reason:&str)->Value {json!({"ok":false,"schema_version":1,"reason":reason})}
fn document(root:&Path)->Option<(String,Value)> {
    let text=mustard_core::io::fs::read_to_string(root.join("mustard.json")).ok()?;
    if text.len()>1_048_576 {return None;}
    let value:Value=serde_json::from_str(&text).ok()?;
    value.is_object().then_some((text,value))
}
fn revision(root:&Path,text:&str)->String {
    let mut hash=blake3::Hasher::new();hash.update(text.as_bytes());hash.update(b"\0host-settings\0");
    if let Ok(bytes)=mustard_core::io::fs::read(root.join(".claude/settings.local.json")) {hash.update(&bytes);}
    hash.finalize().to_hex().to_string()
}
fn read(root:&Path)->Value {
    let Some((text,_))=document(root) else {return refused("settings-unreadable");};
    let config=ProjectConfig::load(root);
    if config.unreadable {return refused("settings-unreadable");}
    let mut values=json!({"enabled":config.enabled(),"rtk":config.rtk(),"vectors":config.ai_vectors_enabled(),
        "jev":config.ai_fallback_enabled(),"agent_model":config.agent_model(),"agent_effort":config.agent_effort(),"search_answer":config.search_answer()});
    values["session_claude_model"]=json!(config.ai.session_models.get("claude").map_or("haiku",String::as_str));
    values["session_codex_model"]=json!(config.ai.session_models.get("codex").map_or("gpt-6-luna",String::as_str));
    for (field,purpose) in PURPOSES {values[field]=json!(config.judgement_filter(purpose)==FilterSetting::Jev);}
    values["build_command"]=json!(config.build_command);values["test_command"]=json!(config.test_command);values["lint_command"]=json!(config.lint_command);
    values["max_compiling_waves"]=json!(crate::commands::flow::round::queue::max_parallel(root));
    let switches=mustard_core::Switches::read(root);
    json!({"ok":true,"schema_version":1,"revision":revision(root,&text),"values":values,
        "jev":super::jev_usage::configuration(root,&config,std::env::var(mustard_core::io::jev_gate::KEY_ENV).ok()),
        "rtk":{"hook_configured":switches.rtk_hook},
        "index_present":mustard_core::io::project_map::model_path(root).is_file(),
        "availability":crate::shared::integration_health::snapshot(root)})
}

fn valid(field:&str,value:&Value)->bool {
    match field {
        "agent_model" | "session_claude_model" | "session_codex_model"=>value.as_str().is_some_and(|name|name.len()<=160 && !name.is_empty() && name.chars().all(|c|c.is_ascii_alphanumeric() || matches!(c,'.'|'_'|':'|'/'|'-'|'['|']'))),
        "agent_effort"=>value.as_str().is_some_and(|effort|AGENT_EFFORTS.contains(&effort)),
        "build_command" | "test_command" | "lint_command"=>value.is_null() || value.as_str().is_some_and(|command|
            !command.trim().is_empty() && command.chars().count()<=1_000 && !command.contains(['\n','\r','\0'])),
        "max_compiling_waves"=>value.as_u64().is_some_and(|waves|(1..=16).contains(&waves)),
        _=>FIELDS.contains(&field) && value.is_boolean(),
    }
}

fn put(value:&mut Value,path:&[&str],replacement:Value)->bool {
    let Some(object)=value.as_object_mut() else {return false;};
    if path.len()==1 {object.insert(path[0].into(),replacement);true}
    else {put(object.entry(path[0]).or_insert_with(||json!({})),&path[1..],replacement)}
}

fn apply(root:&Path,text:&str)->Value {
    if text.len()>16_384 {return refused("settings-invalid-request");}
    let Ok(request)=serde_json::from_str::<Value>(text) else {return refused("settings-invalid-request");};
    if request["schema_version"]!=1 || request["revision"].as_str().is_none() || !request.is_object()
        || request.as_object().is_some_and(|object|object.keys().any(|key|!["schema_version","revision","changes"].contains(&key.as_str()))) {
        return refused("settings-invalid-request");
    }
    let Some(changes)=request["changes"].as_object().filter(|changes|!changes.is_empty()) else {return refused("settings-empty-patch");};
    // Do not echo supplied values (or unknown field names, which may be secrets).
    let invalid:Vec<&str>=changes.iter().filter(|(field,value)|!valid(field,value)).map(|(field,_)|
        if FIELDS.contains(&field.as_str()) {field.as_str()}else{"unsupported-field"}).collect();
    if !invalid.is_empty() {return json!({"ok":false,"schema_version":1,"reason":"settings-invalid-fields","fields":invalid});}
    let Ok(Some(_lock))=mustard_core::io::fs::lock::LockedFile::exclusive_if_free(&root.join(".claude/mustard/settings.lock")) else {return refused("settings-busy");};
    let Some((before,mut next))=document(root) else {return refused("settings-unreadable");};
    if request["revision"]!=revision(root,&before) {return refused("settings-conflict");}
    for (field,value) in changes {
        let path:Vec<&str>=match field.as_str() {
            "vectors"=>vec!["ai","vectors"],"jev"=>vec!["ai","fallback"],
            "agent_model"=>vec!["agents","model"],
            "session_claude_model"=>vec!["ai","sessionModels","claude"],
            "session_codex_model"=>vec!["ai","sessionModels","codex"],"agent_effort"=>vec!["agents","effort"],
            "search_answer"=>vec!["search","answer"],"max_compiling_waves"=>vec!["maxCompilingWaves"],
            _=>vec![field.as_str()],
        };
        let ok=if let Some((old,key))=COMMANDS.iter().find(|(name,_)|name==field) {
            // O nome antigo sai junto, e `null` tira a chave em vez de gravar `null`.
            next.as_object_mut().is_some_and(|object|{
                object.shift_remove(*old);object.shift_remove(*key);
                if !value.is_null() {object.insert((*key).into(),value.clone());}
                true
            })
        }else if let Some((_,purpose))=PURPOSES.iter().find(|(name,_)|name==field) {
            put(&mut next,&["judgement",purpose,"filter"],json!(if value==true {"jev"}else{"none"}))
        }else {put(&mut next,&path,value.clone())};
        if !ok {return refused("settings-invalid-section");}
    }
    let Ok(config)=serde_json::from_value::<ProjectConfig>(next.clone()) else {return refused("settings-invalid-section");};
    // Prepare derived writes before changing anything. Invalid host settings
    // cannot erase hooks or private environment choices.
    let local_path=root.join(".claude/settings.local.json");
    let local=if changes.contains_key("rtk") {
        let Ok(raw)=mustard_core::io::fs::read_to_string(&local_path) else {return refused("settings-local-missing");};
        let Ok(Value::Object(mut object))=serde_json::from_str::<Value>(&raw) else {return refused("settings-local-invalid");};
        mustard_core::platform::project_seed::settings::apply_rtk_hook(&mut object,config.rtk());
        if mustard_core::platform::project_seed::settings::rtk_hook_present(&object)!=config.rtk() {return refused("settings-local-invalid");}
        Some(Value::Object(object))
    }else {None};
    let Ok(mut bytes)=serde_json::to_vec_pretty(&next) else {return refused("settings-write-failed");};
    bytes.push(b'\n');
    if document(root).is_none_or(|(now,_)|request["revision"]!=revision(root,&now)) {return refused("settings-conflict");}
    if mustard_core::io::fs::write_atomic(root.join("mustard.json"),&bytes).is_err() {return refused("settings-write-failed");}
    let mut pending=Vec::new();
    if let Some(local)=local
        && serde_json::to_vec_pretty(&local).ok().is_none_or(|bytes|mustard_core::io::fs::write_atomic(&local_path,&bytes).is_err()) {pending.push("rtk-hook");}
    if (changes.contains_key("agent_model") || changes.contains_key("agent_effort"))
        && (!root.join(".claude/agents/mustard").is_dir() || mustard_core::refresh_agent_texts(root).is_err()) {pending.push("agent-files");}
    if before.as_bytes()!=bytes {
        // A receipt from before these choices cannot certify this configuration.
        let path=root.join(".claude/mustard/installation.json");
        if let Ok(raw)=mustard_core::io::fs::read_to_string(&path)
            && let Ok(Value::Object(mut receipt))=serde_json::from_str::<Value>(&raw) {
                receipt.insert("complete".into(),json!(false));
                receipt.insert("settings_changed".into(),json!(true));
                if serde_json::to_vec_pretty(&receipt).ok().is_none_or(|bytes|mustard_core::io::fs::write_atomic(&path,&bytes).is_err()) {pending.push("installation-receipt");}
        }
    }
    let mut result=read(root);
    result["saved"]=json!(true);
    result["ok"]=json!(pending.is_empty());
    result["pending"]=json!(pending);
    if !pending.is_empty() {result["reason"]=json!("settings-apply-incomplete");}
    result["scan_recommended"]=json!(changes.get("vectors")==Some(&Value::Bool(true)));
    result["agent_changes_apply_to"]=json!("new-agents-only");
    result
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)] // Temporary files and synthetic credentials are test fixtures.
    use super::*;
    const SECRET:&str="synthetic-settings-private-key";
    fn fixture()->tempfile::TempDir {
        let dir=tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude/mustard")).unwrap();
        std::fs::create_dir_all(dir.path().join(".claude/agents/mustard")).unwrap();
        std::fs::write(dir.path().join("mustard.json"),json!({"jev":{"key":SECRET},"future":{"keep":17},"ai":{"extra":"keep"},
            "agents":{"model":"custom-model-1","extra":"keep"},"judgement":{"context":{"extra":"keep"}}}).to_string()).unwrap();
        std::fs::write(dir.path().join(".claude/settings.local.json"),json!({"env":{"TYPESAFE_API_KEY":SECRET},"permissions":{"deny":["keep"]},
            "hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"other hook"}]}]}}).to_string()).unwrap();
        std::fs::write(dir.path().join(".claude/mustard/installation.json"),r#"{"complete":true,"checks":[]}"#).unwrap();
        dir
    }
    fn patch(root:&Path,changes:Value)->Value {
        apply(root,&json!({"schema_version":1,"revision":read(root)["revision"],"changes":changes}).to_string())
    }
    #[test]
    fn reading_is_a_safe_non_mutating_projection() {
        let dir=fixture();let root=dir.path();
        let before=std::fs::read(root.join("mustard.json")).unwrap();
        let shown=read(root);
        assert_eq!(shown["ok"],true);assert_eq!(shown["values"]["agent_model"],"custom-model-1");
        assert_eq!(shown["values"]["vectors"],true);assert_eq!(shown["values"]["jev"],false);
        assert_eq!(shown["jev"]["provider_verified"],false);
        assert!(!shown.to_string().contains(SECRET));assert!(shown.get("future").is_none());
        assert_eq!(std::fs::read(root.join("mustard.json")).unwrap(),before);
        assert!(!root.join(".claude/judgements").exists());assert!(!root.join(".claude/mustard/settings.lock").exists());
    }
    #[test]
    fn explicit_patch_preserves_secrets_other_choices_and_updates_real_hooks_and_agents() {
        let dir=fixture();let root=dir.path();
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            for path in ["mustard.json",".claude/settings.local.json"] {std::fs::set_permissions(root.join(path),std::fs::Permissions::from_mode(0o600)).unwrap();}
        }
        let saved=patch(root,json!({"rtk":true,"jev":true,"jev_search":true,"jev_context":true,"vectors":false,"agent_model":"opus","agent_effort":"high","search_answer":false}));
        assert_eq!(saved["ok"],true,"{saved}");assert_eq!(saved["saved"],true);
        assert!(!saved.to_string().contains(SECRET));
        let (_,value)=document(root).unwrap();
        assert_eq!(value["jev"]["key"],SECRET);assert_eq!(value["future"]["keep"],17);
        assert_eq!(value["ai"]["extra"],"keep");assert_eq!(value["agents"]["extra"],"keep");
        assert_eq!(value["judgement"]["context"]["extra"],"keep");
        assert_eq!(value["judgement"]["search"]["filter"],"jev");assert_eq!(value["ai"]["vectors"],false);
        let local=std::fs::read_to_string(root.join(".claude/settings.local.json")).unwrap();
        assert!(local.contains(SECRET) && local.contains("other hook") && local.contains("rtk hook claude"));
        let agent=std::fs::read_to_string(root.join(".claude/agents/mustard/wave.md")).unwrap();
        assert!(agent.contains("model: opus") && agent.contains("effort: high"));
        let receipt:Value=serde_json::from_str(&std::fs::read_to_string(root.join(".claude/mustard/installation.json")).unwrap()).unwrap();
        assert_eq!(receipt["complete"],false);assert_eq!(receipt["settings_changed"],true);
        let off=patch(root,json!({"rtk":false,"jev_search":false,"enabled":false}));
        assert_eq!(off["ok"],true);assert_eq!(off["values"]["enabled"],false);
        let local=std::fs::read_to_string(root.join(".claude/settings.local.json")).unwrap();
        assert!(!local.contains("rtk hook claude") && local.contains("other hook"));
        assert_eq!(document(root).unwrap().1["judgement"]["search"]["filter"],"none");
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            for path in ["mustard.json",".claude/settings.local.json"] {assert_eq!(std::fs::metadata(root.join(path)).unwrap().permissions().mode() & 0o777,0o600);}
        }
        assert!(!root.join(".claude/judgements").exists());assert!(!root.join(".claude/grain.db").exists());
    }
    #[test]
    fn conflicting_edits_and_invalid_fields_change_nothing() {
        let dir=fixture();let root=dir.path();
        let request=json!({"schema_version":1,"revision":read(root)["revision"],"changes":{"vectors":false}});
        let before=std::fs::read_to_string(root.join("mustard.json")).unwrap()+"\n";
        std::fs::write(root.join("mustard.json"),&before).unwrap();
        assert_eq!(apply(root,&request.to_string())["reason"],"settings-conflict");
        let invalid=patch(root,json!({"vectors":"yes","agent_model":"opus\npermissionMode: bypassPermissions","agent_effort":"ultra",SECRET:SECRET}));
        assert_eq!(invalid["reason"],"settings-invalid-fields");assert_eq!(invalid["fields"].as_array().unwrap().len(),4);
        assert!(!invalid.to_string().contains(SECRET));assert_eq!(std::fs::read_to_string(root.join("mustard.json")).unwrap(),before);
    }
    #[test]
    fn editing_private_host_settings_invalidates_an_open_form() {
        let dir=fixture();let root=dir.path();
        let request=json!({"schema_version":1,"revision":read(root)["revision"],"changes":{"rtk":true}});
        let before=std::fs::read(root.join("mustard.json")).unwrap();
        let local="{\"env\":{\"USER_CHOICE\":\"new\"}}";
        std::fs::write(root.join(".claude/settings.local.json"),local).unwrap();
        assert_eq!(apply(root,&request.to_string())["reason"],"settings-conflict");
        assert_eq!(std::fs::read(root.join("mustard.json")).unwrap(),before);
        assert_eq!(std::fs::read_to_string(root.join(".claude/settings.local.json")).unwrap(),local);
    }
    #[test]
    fn invalid_local_hooks_refuse_the_entire_patch_before_writing() {
        let dir=fixture();let root=dir.path();let before=std::fs::read(root.join("mustard.json")).unwrap();
        std::fs::write(root.join(".claude/settings.local.json"),r#"{"hooks":[]}"#).unwrap();
        assert_eq!(patch(root,json!({"rtk":true,"vectors":false}))["reason"],"settings-local-invalid");
        assert_eq!(std::fs::read(root.join("mustard.json")).unwrap(),before);
        std::fs::write(root.join("mustard.json"),"{broken").unwrap();
        assert_eq!(read(root)["reason"],"settings-unreadable");
    }
    /// Os três comandos mudam sem tocar na chave nem nos outros campos;
    /// `null` tira a chave, e um comando com quebra de linha não muda nada.
    #[test]
    fn a_command_patch_keeps_the_key_and_every_other_field() {
        let dir=fixture();let root=dir.path();
        let saved=patch(root,json!({"build_command":"pnpm build","test_command":"dotnet test","lint_command":"npx eslint ."}));
        assert_eq!(saved["ok"],true,"{saved}");
        assert_eq!(saved["values"]["test_command"],"dotnet test");assert!(!saved.to_string().contains(SECRET));
        let (_,value)=document(root).unwrap();
        assert_eq!(value["buildCommand"],"pnpm build");assert_eq!(value["lintCommand"],"npx eslint .");
        assert_eq!(value["jev"]["key"],SECRET);assert_eq!(value["future"]["keep"],17);
        assert_eq!(value["agents"]["model"],"custom-model-1");assert_eq!(value["agents"]["extra"],"keep");
        assert_eq!(patch(root,json!({"lint_command":null}))["ok"],true);
        let (before,value)=document(root).unwrap();
        assert!(value.get("lintCommand").is_none() && value["buildCommand"]=="pnpm build","{value}");
        let refused=patch(root,json!({"test_command":"dotnet test\nrm -rf ."}));
        assert_eq!(refused["reason"],"settings-invalid-fields");assert_eq!(refused["fields"],json!(["test_command"]));
        assert_eq!(std::fs::read_to_string(root.join("mustard.json")).unwrap(),before);
    }
    /// O teto de ondas em paralelo gravado aparece na leitura seguinte, com a
    /// chave intacta; fora de 1 a 16 ele é recusado sem mudar o arquivo.
    #[test]
    fn a_wave_cap_patch_changes_the_value_read_next() {
        let dir=fixture();let root=dir.path();
        assert_eq!(read(root)["values"]["max_compiling_waves"],4);
        assert_eq!(patch(root,json!({"max_compiling_waves":7}))["ok"],true);
        assert_eq!(read(root)["values"]["max_compiling_waves"],7);
        assert_eq!(document(root).unwrap().1["jev"]["key"],SECRET);
        let before=std::fs::read_to_string(root.join("mustard.json")).unwrap();
        for waves in [json!(0),json!(17),json!("7")] {
            assert_eq!(patch(root,json!({"max_compiling_waves":waves}))["reason"],"settings-invalid-fields","{waves}");
        }
        assert_eq!(std::fs::read_to_string(root.join("mustard.json")).unwrap(),before);
    }
    #[test]
    fn a_busy_writer_and_wrong_versions_are_refused_without_app_commands() {
        let dir=fixture();let root=dir.path();
        let _lock=mustard_core::io::fs::lock::LockedFile::exclusive(&root.join(".claude/mustard/settings.lock")).unwrap();
        assert_eq!(patch(root,json!({"jev":true}))["reason"],"settings-busy");
        assert_eq!(apply(root,r#"{"schema_version":2,"revision":"x","changes":{"jev":true}}"#)["reason"],"settings-invalid-request");
    }
}
