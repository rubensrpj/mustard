//! Native per-request receipts. These are a separate live view, never added to
//! transcript totals. Turn/index keys deduplicate replayed events after reload.
use serde_json::{Value,json};
use std::path::{Path,PathBuf};
pub(super) fn file(root:&Path,session:&str)->PathBuf {root.join(".claude/mustard/usage/live").join(format!("{}.json",blake3::hash(session.as_bytes())))}
pub(super) fn record(start:&Path,session:&str,input:&Value)->Value {
    let root=mustard_core::io::spec_events::spec_root(start);
    if !mustard_core::ProjectConfig::exists(&root) {return json!({"ok":true,"recorded":false});}
    if session.is_empty() || session.len()>200 {return json!({"ok":false,"reason":"usage-invalid-session"});}
    let result=(||->Result<(),&str>{
        let mut held=mustard_core::io::fs::lock::LockedFile::exclusive_if_free(&file(&root,session)).map_err(|_|"live-usage-busy")?.ok_or("live-usage-busy")?;
        let raw=held.read_to_string().map_err(|_|"live-usage-unreadable")?;
        if raw.len()>2_000_000 {return Err("live-usage-capacity");}
        let mut data=if raw.is_empty(){json!({"schema_version":1,"agents":{},"steps":{},"main":{}})}else{serde_json::from_str::<Value>(&raw).map_err(|_|"live-usage-unreadable")?};
        if data["schema_version"]!=1 || !data["agents"].is_object() || !data["steps"].is_object() {return Err("live-usage-unreadable");}
        let now=chrono::Utc::now().to_rfc3339();
        if let Some(context)=input.get("context").filter(|v|v.is_object()) {
            let mut projected=json!({});
            for field in ["tokens","window","percent","percentUsed"] {if let Some(value)=context.get(field).filter(|value|value.is_number()) {projected[field]=value.clone();}}
            data["context"]=projected;
        }
        if let Some(limits)=input["rateLimits"].as_array() {data["rateLimits"]=json!(limits.iter().take(16).map(|limit|{
            json!({"kind":limit["kind"].as_str().filter(|s|s.len()<=80),"percentUsed":limit["percentUsed"].as_f64(),"resetsAt":limit["resetsAt"].as_str().filter(|s|s.len()<=80)})
        }).collect::<Vec<_>>());}
        let actor=input["agent_id"].as_str().filter(|id|!id.is_empty() && id.len()<=200).unwrap_or("main");
        let key=blake3::hash(actor.as_bytes()).to_hex().to_string();
        let mut row=if actor=="main"{data["main"].clone()}else{data["agents"][&key].clone()};
        if !row.is_object() {row=json!({});}
        row["id"]=json!(actor);
        if row["started_at"].is_null(){row["started_at"]=json!(now);}
        for field in ["model","effort","agent_type"] {
            if let Some(value)=input[field].as_str().filter(|v|v.len()<=160 && !v.chars().any(char::is_control)) {row[field]=json!(value);}
        }
        if input["kind"]=="spawn" {row["status"]=json!("running");}
        if input["kind"]=="complete" {row["status"]=json!(if input["aborted"]==true{"interrupted"}else{"returned"});row["ended_at"]=json!(now);}
        if input["kind"]=="step" {
            let turn=input["turn_id"].as_str().filter(|s|s.len()<=200).ok_or("usage-invalid-turn")?;
            let index=input["index"].as_u64().ok_or("usage-invalid-step")?;
            let step=blake3::hash(format!("{actor}:{turn}:{index}").as_bytes()).to_hex().to_string();
            let fields=["input_tokens","output_tokens","cache_read_input_tokens","cache_creation_input_tokens"];
            let mut tokens=json!({});
            for field in fields {if let Some(n)=input["usage"][field].as_u64().filter(|n|*n<=1_000_000_000){tokens[field]=json!(n);}}
            if tokens.as_object().is_none_or(|fields|fields.is_empty()) {return Ok(());}
            let previous=&data["steps"][&step];
            if previous.is_null() {
                if data["steps"].as_object().is_some_and(|steps|steps.len()>=8192) {return Err("live-usage-capacity");}
                for field in fields {if let Some(n)=tokens[field].as_u64(){row[field]=json!(row[field].as_u64().unwrap_or(0).saturating_add(n));}}
                row["requests"]=json!(row["requests"].as_u64().unwrap_or(0)+1);
                let context=["input_tokens","cache_read_input_tokens","cache_creation_input_tokens"];
                row["context_tokens"]=json!(context.iter().map(|field|tokens[*field].as_u64()).sum::<Option<u64>>());
                row["status"]=json!("running");row["last_step_at"]=json!(now);
                data["steps"][&step]=tokens;
            }else if previous!=&tokens {return Err("usage-step-conflict");}
        }
        if actor=="main" {data["main"]=row;}else{data["agents"][key]=row;}
        data["measured_at"]=json!(now);data["origin"]=json!("native-host-events");
        held.replace(data.to_string().as_bytes()).map_err(|_|"live-usage-write-failed")?;Ok(())
    })();
    result.map_or_else(|reason|json!({"ok":false,"reason":reason}),|()|json!({"ok":true,"recorded":true}))
}
pub(super) fn snapshot(root:&Path,session:Option<&str>)->Value {
    let Some(session)=session else {return Value::Null;};
    let mut value=mustard_core::io::fs::lock::read_shared(&file(root,session)).ok()
        .and_then(|raw|serde_json::from_str::<Value>(&raw).ok()).unwrap_or(Value::Null);
    if value.is_null(){return value;}
    if let Some(fields)=value.as_object_mut(){fields.remove("steps");}
    if let Some(agents)=value["agents"].as_object_mut(){for row in agents.values_mut(){
        let input=serde_json::from_value::<mustard_core::domain::model::contract::HookInput>(json!({"session_id":session,"agent_id":row["id"]}));
        if let Ok(input)=input && let Some((spec,wave,dispatch))=crate::shared::host_observation::tool_binding(root,"claude",&input) {
            row["spec"]=json!(spec);row["wave"]=json!(wave);row["dispatch"]=json!(dispatch);
        }
    }}
    super::safe_value(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_replays_are_deduplicated_and_costless_measurements_survive() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
        let step=json!({"kind":"step","agent_id":"a","turn_id":"t","index":0,"model":"fixture","usage":{"input_tokens":20,"output_tokens":5,"cache_read_input_tokens":100}});
        for _ in 0..2 {assert_eq!(record(root,"s",&step)["ok"],true);}
        assert_eq!(record(root,"s",&json!({"context":{"tokens":125,"window":200_000},"rateLimits":[]}))["ok"],true);
        let view=snapshot(root,Some("s"));let row=view["agents"].as_object().unwrap().values().next().unwrap();
        assert_eq!(row["input_tokens"],20);assert_eq!(row["requests"],1);assert_eq!(view["context"]["tokens"],125);
        assert!(view.get("steps").is_none());assert!(snapshot(root,Some("other")).is_null());
    }
}
