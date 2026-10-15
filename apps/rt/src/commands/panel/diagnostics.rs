//! Bounded, sanitized diagnostic events. Never stores tool results, prompts,
//! command bodies or credentials. Export is a local snapshot, never upload.
use serde_json::{Value,json};
use std::path::Path;
/// The cause of a failure on one line, without control characters, bounded.
fn short(text:&str)->String {text.split_whitespace().collect::<Vec<_>>().join(" ").chars().filter(|c|!c.is_control()).take(300).collect()}
/// Records one event: component, operation, reason and, when given, the detail
/// of the cause (the disk or parse error). A file that no longer parses (a
/// rewrite cut in the middle) does not stop recording: its text is kept next
/// to it, in a file named by the time, and the list restarts with the event
/// that says so.
pub(super) fn record(root:&Path,input:&Value)->Value {
    if !mustard_core::ProjectConfig::exists(root) {return json!({"ok":true,"recorded":false});}
    let mut event=json!({"at":chrono::Utc::now().to_rfc3339()});
    for key in ["component","operation","reason","spec","wave","session","agent","surface"] {
        if let Some(value)=input.get(key).filter(|v|v.is_u64() || v.as_str().is_some_and(|s|s.len()<=200 && !s.chars().any(char::is_control))) {
            event[key]=value.clone();
        }
    }
    if let Some(detail)=input["detail"].as_str().map(short).filter(|detail|!detail.is_empty()) {event["detail"]=json!(detail);}
    if event["component"].is_null() || event["reason"].is_null() {return json!({"ok":false,"reason":"diagnostic-invalid"});}
    let path=root.join(".claude/mustard/panel/diagnostics.json");
    let result=(||->Result<(),(&str,String)>{
        let mut lock=mustard_core::io::fs::lock::LockedFile::exclusive_if_free(&path).map_err(|e|("diagnostics-busy",e.to_string()))?
            .ok_or(("diagnostics-busy","another recording holds the diagnostics file".to_string()))?;
        let raw=lock.read_to_string().map_err(|e|("diagnostics-unreadable",e.to_string()))?;
        let mut events=match if raw.is_empty(){Ok(Vec::new())}else{serde_json::from_str::<Vec<Value>>(&raw)} {
            Ok(events)=>events,
            Err(error)=>{
                let kept=path.with_file_name(format!("diagnostics.unreadable-{}.json",chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ")));
                mustard_core::io::fs::write_atomic(&kept,raw.as_bytes()).map_err(|e|("diagnostics-unreadable",format!("{error}; not kept: {e}")))?;
                let name=kept.file_name().map(|name|name.to_string_lossy().into_owned()).unwrap_or_default();
                vec![json!({"at":event["at"],"component":"panel","operation":"diagnostics","reason":"diagnostics-unreadable","detail":short(&format!("{error}; previous text kept in {name}"))})]
            }
        };
        let event=super::safe_value(event);
        if let Some(last)=events.last_mut().filter(|last|last["component"]==event["component"] && last["operation"]==event["operation"] && last["reason"]==event["reason"] && last["session"]==event["session"]) {
            last["count"]=json!(last["count"].as_u64().unwrap_or(1)+1);last["at"]=event["at"].clone();
            if !event["detail"].is_null() {last["detail"]=event["detail"].clone();}
        }else{events.push(event);}
        if events.len()>128 {events.drain(..events.len()-128);}
        lock.replace(json!(events).to_string().as_bytes()).map_err(|e|("diagnostics-write-failed",e.to_string()))
    })();
    result.map_or_else(|(reason,detail)|json!({"ok":false,"reason":reason,"detail":short(&detail)}),|()|json!({"ok":true}))
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
#[cfg(test)]
mod tests {
    use super::*;
    /// A diagnostics file cut in the middle, as the full disk left it, no
    /// longer stops recording: its text stays aside, the list restarts saying
    /// so, and the new failure keeps the cause it was sent with.
    #[test]
    fn an_unreadable_diagnostics_file_is_kept_aside_and_recording_resumes_with_the_cause() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
        let folder=root.join(".claude/mustard/panel");std::fs::create_dir_all(&folder).unwrap();
        let cut=r#"[{"component":"usage","operation":"record","reason":"live-u"#;
        std::fs::write(folder.join("diagnostics.json"),cut).unwrap();
        let failure=json!({"component":"panel","operation":"refresh","reason":"panel-query-failed","detail":"exit 1:\n  database is locked","session":"s"});
        assert_eq!(record(root,&failure)["ok"],true);
        let kept:Vec<_>=std::fs::read_dir(&folder).unwrap().filter_map(Result::ok).map(|entry|entry.path())
            .filter(|path|path.file_name().unwrap().to_string_lossy().starts_with("diagnostics.unreadable-")).collect();
        assert_eq!(kept.len(),1);assert_eq!(std::fs::read_to_string(&kept[0]).unwrap(),cut);
        let events:Vec<Value>=serde_json::from_str(&std::fs::read_to_string(folder.join("diagnostics.json")).unwrap()).unwrap();
        let reasons:Vec<_>=events.iter().map(|event|(event["reason"].clone(),event["detail"].as_str().is_some())).collect();
        assert_eq!(reasons,[(json!("diagnostics-unreadable"),true),(json!("panel-query-failed"),true)]);
        assert_eq!(events[1]["detail"],"exit 1: database is locked");
    }
}
