//! Bounded, sanitized diagnostic events. Never stores tool results, prompts,
//! command bodies or credentials. Export is a local snapshot, never upload.
use serde_json::{Value,json};
use std::path::Path;
pub(super) fn record(root:&Path,input:&Value)->Value {
    if !mustard_core::ProjectConfig::exists(root) {return json!({"ok":true,"recorded":false});}
    let mut event=json!({"at":chrono::Utc::now().to_rfc3339()});
    for key in ["component","operation","reason","spec","wave","session","agent","surface"] {
        if let Some(value)=input.get(key).filter(|v|v.is_u64() || v.as_str().is_some_and(|s|s.len()<=200 && !s.chars().any(char::is_control))) {
            event[key]=value.clone();
        }
    }
    if event["component"].is_null() || event["reason"].is_null() {return json!({"ok":false,"reason":"diagnostic-invalid"});}
    let path=root.join(".claude/mustard/panel/diagnostics.json");
    let result=(||->Result<(),&str>{
        let mut lock=mustard_core::io::fs::lock::LockedFile::exclusive_if_free(&path).map_err(|_|"diagnostics-busy")?.ok_or("diagnostics-busy")?;
        let raw=lock.read_to_string().map_err(|_|"diagnostics-unreadable")?;
        let mut events=if raw.is_empty(){Vec::<Value>::new()}else{serde_json::from_str(&raw).map_err(|_|"diagnostics-unreadable")?};
        let event=super::safe_value(event);
        if let Some(last)=events.last_mut().filter(|last|last["component"]==event["component"] && last["operation"]==event["operation"] && last["reason"]==event["reason"] && last["session"]==event["session"]) {
            last["count"]=json!(last["count"].as_u64().unwrap_or(1)+1);last["at"]=event["at"].clone();
        }else{events.push(event);}
        if events.len()>128 {events.drain(..events.len()-128);}
        lock.replace(json!(events).to_string().as_bytes()).map_err(|_|"diagnostics-write-failed")
    })();
    result.map_or_else(|reason|json!({"ok":false,"reason":reason}),|()|json!({"ok":true}))
}
pub(super) fn snapshot(root:&Path)->Value {
    let events=mustard_core::io::fs::lock::read_shared(&root.join(".claude/mustard/panel/diagnostics.json")).ok()
        .and_then(|raw|serde_json::from_str::<Value>(&raw).ok()).unwrap_or_else(||json!([]));
    let mut events=events.as_array().cloned().unwrap_or_default();
    // Existing ledgers remain authoritative: merge failure metadata for the
    // diagnostic view instead of copying private commands or tool results.
    for (component,path) in [("jev",root.join(".claude/judgements/attempts.ndjson"))] {
        if let Ok(raw)=std::fs::read_to_string(path) {for event in raw.lines().rev().take(128).filter_map(|line|serde_json::from_str::<Value>(line).ok()) {
            if event["status"].as_u64().is_some_and(|status|status>=400) || event["ok"]==false || event["result"]=="refused" {
                events.push(json!({"component":component,"operation":event["purpose"],"reason":event["reason"],"status":event["status"],"at":event["at"],"spec":event["spec"]}));
            }
        }}
    }
    for (name,log) in mustard_core::io::spec_index::read_specs(root) {
        let operations=crate::commands::spec_events::conversation::recorded_operations(root,&name);
        for event in log.events.iter().chain(&operations.events).rev().filter(|event|event.event_type=="call" && event.str_field("result")==Some("refused")).take(24) {
            events.push(json!({"component":"runtime","operation":event.str_field("command"),"reason":event.str_field("refusal"),"spec":name,"wave":event.wave(),"at":event.at()}));
        }
    }
    events.sort_by(|a,b|a["at"].as_str().cmp(&b["at"].as_str()));
    if events.len()>256 {events.drain(..events.len()-256);}
    super::safe_value(json!({"events":events,"health":crate::shared::integration_health::snapshot(root),
        "plugin_loaders":crate::shared::host_installation::snapshot(root),"local_only":true,"model_calls":0}))
}
