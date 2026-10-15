//! Native per-request receipts. These are a separate live view, never added to
//! transcript totals. Turn/index keys deduplicate replayed events after reload.
use serde_json::{Value,json};
use std::path::{Path,PathBuf};
pub(super) fn file(root:&Path,session:&str)->PathBuf {root.join(".claude/mustard/usage/live").join(format!("{}.json",blake3::hash(session.as_bytes())))}
pub(super) fn record(start:&Path,session:&str,input:&Value)->Value {
    let root=mustard_core::io::spec_events::spec_root(start);
    if !mustard_core::ProjectConfig::exists(&root) {return json!({"ok":true,"recorded":false});}
    if session.is_empty() || session.len()>200 {return json!({"ok":false,"reason":"usage-invalid-session"});}
    let fail=|reason:&'static str,detail:String|(reason,detail);
    let result=(||->Result<(),(&'static str,String)>{
        let mut held=mustard_core::io::fs::lock::LockedFile::exclusive_if_free(&file(&root,session)).map_err(|e|fail("live-usage-busy",e.to_string()))?
            .ok_or_else(||fail("live-usage-busy","another recording holds the live usage file".into()))?;
        let raw=held.read_to_string().map_err(|e|fail("live-usage-unreadable",e.to_string()))?;
        if raw.len()>2_000_000 {return Err(fail("live-usage-capacity",format!("{} bytes, limit 2000000",raw.len())));}
        let mut data=if raw.is_empty(){json!({"schema_version":1,"agents":{},"steps":{},"main":{}})}else{serde_json::from_str::<Value>(&raw).map_err(|e|fail("live-usage-unreadable",e.to_string()))?};
        if data["schema_version"]!=1 || !data["agents"].is_object() || !data["steps"].is_object() {
            return Err(fail("live-usage-unreadable","schema_version 1 with agents and steps objects expected".into()));
        }
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
            let turn=input["turn_id"].as_str().filter(|s|s.len()<=200).ok_or_else(||fail("usage-invalid-turn","turn_id missing or longer than 200".into()))?;
            let index=input["index"].as_u64().ok_or_else(||fail("usage-invalid-step","index missing or not a number".into()))?;
            let step=blake3::hash(format!("{actor}:{turn}:{index}").as_bytes()).to_hex().to_string();
            let fields=["input_tokens","output_tokens","cache_read_input_tokens","cache_creation_input_tokens"];
            let mut tokens=json!({});
            for field in fields {if let Some(n)=input["usage"][field].as_u64().filter(|n|*n<=1_000_000_000){tokens[field]=json!(n);}}
            if tokens.as_object().is_none_or(|fields|fields.is_empty()) {return Ok(());}
            let previous=&data["steps"][&step];
            if previous.is_null() {
                if data["steps"].as_object().is_some_and(|steps|steps.len()>=8192) {return Err(fail("live-usage-capacity","8192 steps already recorded".into()));}
                for field in fields {if let Some(n)=tokens[field].as_u64(){row[field]=json!(row[field].as_u64().unwrap_or(0).saturating_add(n));}}
                row["requests"]=json!(row["requests"].as_u64().unwrap_or(0)+1);
                let context=["input_tokens","cache_read_input_tokens","cache_creation_input_tokens"];
                row["context_tokens"]=json!(context.iter().map(|field|tokens[*field].as_u64()).sum::<Option<u64>>());
                row["status"]=json!("running");row["last_step_at"]=json!(now);
                data["steps"][&step]=tokens;
            }else if previous!=&tokens {return Err(fail("usage-step-conflict",format!("step {turn}:{index} already recorded with other tokens")));}
        }
        if actor=="main" {data["main"]=row;}else{data["agents"][key]=row;}
        data["measured_at"]=json!(now);data["origin"]=json!("native-host-events");
        held.replace(data.to_string().as_bytes()).map_err(|e|fail("live-usage-write-failed",e.to_string()))?;Ok(())
    })();
    result.map_or_else(|(reason,detail)|{
        // The failure enters the diagnostics with its cause whoever sent the
        // measurement; `diagnosed` tells the caller not to record it again.
        let event=json!({"component":"usage","operation":"record","reason":reason,"detail":detail,"session":session});
        let diagnosed=super::diagnostics::record(&root,&event)["ok"]==true;
        json!({"ok":false,"reason":reason,"detail":detail,"diagnosed":diagnosed})
    },|()|json!({"ok":true,"recorded":true}))
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
    /// A live file cut in the middle of a rewrite, as the full disk left it,
    /// makes the measurement fail with the parse error as its cause, and that
    /// cause reaches the diagnostics once, even when the cost was recorded.
    #[test]
    fn a_failing_live_recording_leaves_its_cause_in_the_diagnostics() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
        std::fs::create_dir_all(file(root,"s").parent().unwrap()).unwrap();
        std::fs::write(file(root,"s"),r#"{"schema_version":1,"agents":{"a":{"id":"unterminated"#).unwrap();
        let measurement=json!({"kind":"measure","cost":{"usd":0.5},"model":"fixture"});
        let answer=super::super::consumption::record(root,"s",&measurement);
        assert_eq!((answer["ok"].clone(),answer["live"]["reason"].clone(),answer["live"]["diagnosed"].clone()),(json!(true),json!("live-usage-unreadable"),json!(true)),"{answer}");
        let detail=answer["live"]["detail"].as_str().unwrap();
        assert!(detail.contains("EOF while parsing"),"{detail}");
        let events:Vec<Value>=serde_json::from_str(&std::fs::read_to_string(root.join(".claude/mustard/panel/diagnostics.json")).unwrap()).unwrap();
        let recorded:Vec<_>=events.iter().map(|event|(event["component"].clone(),event["operation"].clone(),event["reason"].clone(),event["detail"].clone(),event["session"].clone())).collect();
        assert_eq!(recorded,[(json!("usage"),json!("record"),json!("live-usage-unreadable"),json!(detail),json!("s"))]);
    }
}
