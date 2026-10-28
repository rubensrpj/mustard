//! Bounded, sanitized diagnostic events. Never stores tool results, prompts,
//! command bodies or credentials, and never leaves the machine.
use mustard_core::domain::text::one_line;
use serde_json::{Value,json};
use std::path::Path;
/// The cause of a failure on one line, without control characters, bounded.
fn short(text:&str)->String {one_line(text).chars().filter(|c|!c.is_control()).take(300).collect()}
/// Records one event: component, operation, reason and, when given, the detail
/// of the cause (the disk or parse error). A file that no longer parses (a
/// rewrite cut in the middle) does not stop recording: its text is kept next
/// to it, in a file named by the time, and the list restarts with the event
/// that says so.
pub(super) fn record(root:&Path,input:&Value)->Value {
    record_within(root,input,super::live_usage::LOCK_PATIENCE)
}

/// [`record`] com a vez de outra gravação do arquivo esperada até `patience`.
fn record_within(root:&Path,input:&Value,patience:std::time::Duration)->Value {
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
        // Dois diagnósticos ao mesmo tempo: o segundo espera o primeiro gravar,
        // por pouco tempo, em vez de se perder.
        let mut lock=mustard_core::io::fs::lock::LockedFile::exclusive_within(&path,patience).map_err(|e|("diagnostics-busy",e.to_string()))?
            .ok_or_else(||("diagnostics-busy",format!("another recording held the diagnostics file for {} ms",patience.as_millis())))?;
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
    /// O diagnóstico que chega enquanto outro grava o arquivo espera ele soltar
    /// e grava o próprio: com a espera de uma hora, a outra gravação solta
    /// muito antes, por mais carregada que a máquina esteja. Só a gravação que
    /// segura o arquivo a espera inteira faz o diagnóstico desistir, com a
    /// causa: com a espera vencida, desiste na hora.
    #[test]
    fn a_diagnostic_waits_for_another_one_recording_the_file() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path().to_path_buf();std::fs::write(root.join("mustard.json"),"{}").unwrap();
        let path=root.join(".claude/mustard/panel/diagnostics.json");
        let failure=json!({"component":"panel","operation":"refresh","reason":"panel-query-failed","session":"s"});
        let other=mustard_core::io::fs::lock::LockedFile::exclusive(&path).unwrap();
        let waiting={let (root,failure)=(root.clone(),failure.clone());std::thread::spawn(move||record_within(&root,&failure,std::time::Duration::from_secs(3600)))};
        drop(other);
        assert_eq!(waiting.join().unwrap(),json!({"ok":true}));
        let events:Vec<Value>=serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(events.iter().map(|event|event["reason"].clone()).collect::<Vec<_>>(),[json!("panel-query-failed")]);
        let _held=mustard_core::io::fs::lock::LockedFile::exclusive(&path).unwrap();
        let answer=record_within(&root,&failure,std::time::Duration::ZERO);
        assert_eq!((answer["ok"].clone(),answer["reason"].clone()),(json!(false),json!("diagnostics-busy")),"{answer}");
    }
}
