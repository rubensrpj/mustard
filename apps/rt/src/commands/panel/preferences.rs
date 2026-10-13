//! Presentation choices only: never changes models, permissions or waves.
use serde_json::{Value,json};
use std::path::Path;
const TABS:&[&str]=&["Projeto","Definições","Ondas","Execução","Consumo","Ideias","Diagnóstico","Configurações"];
fn defaults()->Value {json!({"schema_version":1,"revision":0,"selected":"overview","profiles":[
    {"id":"overview","name":"Visão geral","tab":"Projeto","tabs":["Projeto","Definições","Ondas","Execução","Consumo","Ideias","Diagnóstico","Configurações"]},
    {"id":"planning","name":"Planejamento","tab":"Definições","tabs":["Projeto","Definições","Ideias","Ondas","Configurações"]},
    {"id":"execution","name":"Execução","tab":"Execução","tabs":["Projeto","Execução","Ondas","Consumo","Diagnóstico","Configurações"]},
    {"id":"review","name":"Revisão","tab":"Ondas","tabs":["Projeto","Ondas","Definições","Execução","Diagnóstico","Configurações"]},
    {"id":"usage","name":"Consumo","tab":"Consumo","tabs":["Projeto","Consumo","Diagnóstico","Configurações"]}]})}
fn valid(doc:&Value)->bool {
    let Some(profiles)=doc["profiles"].as_array() else {return false;};
    if doc["schema_version"]!=1 || doc["revision"].as_u64().is_none() || profiles.is_empty() || profiles.len()>16 {return false;}
    let mut ids=std::collections::BTreeSet::new();
    profiles.iter().all(|profile| {
        if profile.as_object().is_none_or(|fields|fields.keys().any(|key|!["id","name","tab","tabs"].contains(&key.as_str()))) {return false;}
        let Some(id)=profile["id"].as_str() else {return false;};
        let Some(tabs)=profile["tabs"].as_array() else {return false;};
        !id.is_empty() && id.len()<=48 && id.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'-') && ids.insert(id)
        && profile["name"].as_str().is_some_and(|name|!name.trim().is_empty() && name.len()<=100 && !name.chars().any(char::is_control))
        && profile["tab"].as_str().is_some_and(|tab|TABS.contains(&tab) && tabs.iter().any(|value|value==tab))
        && !tabs.is_empty() && tabs.len()<=TABS.len() && tabs.iter().all(|tab|tab.as_str().is_some_and(|tab|TABS.contains(&tab)))
    }) && profiles.iter().any(|profile|profile["id"]==doc["selected"])
}

fn read(path:&Path)->Result<Value,&'static str> {
    match mustard_core::io::fs::lock::read_shared(path) {
        Ok(raw)=>serde_json::from_str::<Value>(&raw).ok().filter(valid).ok_or("panel-preferences-unreadable"),
        Err(_) if !path.exists()=>Ok(defaults()),Err(_)=>Err("panel-preferences-unreadable"),
    }
}
pub(super) fn run(start:&Path,patch:Option<&str>)->Value {
    let root=mustard_core::io::spec_events::spec_root(start);let path=root.join(".claude/mustard/panel/preferences.json");
    let result=(||->Result<Value,&str>{
        let Some(patch)=patch else {return read(&path);};
        if !mustard_core::ProjectConfig::exists(&root) {return Err("project-not-initialized");}
        if patch.len()>32768 {return Err("panel-preferences-invalid");}
        let request:Value=serde_json::from_str(patch).map_err(|_|"panel-preferences-invalid")?;
        if request["schema_version"]!=1 || request.as_object().is_none_or(|object|object.keys().any(|key|!["schema_version","revision","selected","profile","delete"].contains(&key.as_str()))) {return Err("panel-preferences-invalid");}
        let mut lock=mustard_core::io::fs::lock::LockedFile::exclusive_if_free(&path).map_err(|_|"panel-preferences-busy")?.ok_or("panel-preferences-busy")?;
        let raw=lock.read_to_string().map_err(|_|"panel-preferences-unreadable")?;
        let mut next=if raw.trim().is_empty(){defaults()}else{serde_json::from_str::<Value>(&raw).map_err(|_|"panel-preferences-unreadable")?};
        if !valid(&next) {return Err("panel-preferences-unreadable");}
        if request["revision"]!=next["revision"] {return Err("panel-preferences-conflict");}
        if let Some(profile)=request.get("profile") {
            let profiles=next["profiles"].as_array_mut().ok_or("panel-preferences-invalid")?;
            if let Some(existing)=profiles.iter_mut().find(|p|p["id"]==profile["id"]) {*existing=profile.clone();}else{profiles.push(profile.clone());}
        }
        if let Some(id)=request["delete"].as_str() {next["profiles"].as_array_mut().ok_or("panel-preferences-invalid")?.retain(|p|p["id"]!=id);}
        if let Some(selected)=request.get("selected") {next["selected"]=selected.clone();}
        if !valid(&next) {return Err("panel-preferences-invalid");}
        next["revision"]=json!(next["revision"].as_u64().unwrap_or(0)+1);
        lock.replace(next.to_string().as_bytes()).map_err(|_|"panel-preferences-write-failed")?;
        Ok(next)
    })();
    result.map_or_else(|reason|json!({"ok":false,"reason":reason}),|mut data|{data["ok"]=json!(true);data})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_are_versioned_and_cannot_change_execution_or_other_configuration() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();let config="{\"agents\":{\"model\":\"fixture\"}}";
        std::fs::write(root.join("mustard.json"),config).unwrap();
        let initial=run(root,None);assert_eq!(initial["profiles"].as_array().unwrap().len(),5);assert!(!root.join(".claude").exists());
        let patch=json!({"schema_version":1,"revision":0,"selected":"usage"}).to_string();
        let saved=run(root,Some(&patch));assert_eq!(saved["selected"],"usage");
        assert_eq!(run(root,Some(&patch))["reason"],"panel-preferences-conflict");
        assert_eq!(std::fs::read_to_string(root.join("mustard.json")).unwrap(),config);
        let bad=json!({"schema_version":1,"revision":1,"agent_model":"other"});
        assert_eq!(run(root,Some(&bad.to_string()))["reason"],"panel-preferences-invalid");
        assert!(!root.join(".claude/spec").exists());
    }
}
