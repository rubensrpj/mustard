//! Private host-provided transcript locations, bound to an exact dispatch.
//! Never exported to the panel, never treated as approval or a verified fact.
use super::{digest, token};
use mustard_core::{domain::{model::contract::HookInput, spec_events::SpecLog, wave_prompt::ReadDispatch}, io::{fs, transcript}};
use serde_json::{Value, json};
use std::{io::Read, path::{Path, PathBuf}};

pub(crate) struct Measured {
    pub usage:transcript::Usage,
    pub steps:Option<u64>,
}

fn directory(root:&Path)->PathBuf { root.join(".claude/mustard/host-transcripts") }
fn inside(root:&Path,path:&Path)->bool {
    let mut ancestor=path.to_path_buf();
    while !ancestor.exists() { if !ancestor.pop() {return false;} }
    ancestor.canonicalize().ok().zip(root.canonicalize().ok()).is_some_and(|(p,r)|p.starts_with(r))
}
fn read(path:&Path)->Option<Value> {
    if path.symlink_metadata().ok()?.file_type().is_symlink() || path.metadata().ok()?.len()>32_768 {return None;}
    serde_json::from_str(&fs::lock::read_shared(path).ok()?).ok()
}
fn opening(path:&Path)->Option<(String,String)> {
    let mut bytes=Vec::new();
    std::fs::File::open(path).ok()?.take(262_144).read_to_end(&mut bytes).ok()?;
    for line in String::from_utf8_lossy(&bytes).lines() {
        let Ok(row)=serde_json::from_str::<Value>(line) else {continue};
        let message=if row["type"]=="response_item" {&row["payload"]} else {&row["message"]};
        if message["role"]!="user" {continue;}
        let content=&message["content"];
        let text=content.as_str().map(str::to_string).unwrap_or_else(||content.as_array().into_iter().flatten().filter_map(|p|p["text"].as_str()).collect::<Vec<_>>().join("\n"));
        if ReadDispatch::of(&text).is_some() || heading(&text).is_some() {
            return Some((text,row["timestamp"].as_str()?.into()));
        }
    }
    None
}

fn heading(text:&str)->Option<(String,u64)> {
    let line=text.lines().next()?;
    use mustard_core::platform::i18n::Locale;
    mustard_core::domain::wave_prompt::wave_of_title(line,Locale::PtBr)
        .or_else(||mustard_core::domain::wave_prompt::wave_of_title(line,Locale::EnUs))
}

pub(crate) fn tool_binding(root:&Path,host:&str,input:&HookInput)->Option<(String,u64,u64)> {
    let session=input.session_id.as_deref()?;
    let agent=input.agent_id.as_deref()?;
    let record=read(&directory(root).join(format!("{host}-{}-{}.json",digest(session),digest(agent))))?;
    let spec=record["spec"].as_str()?;
    let wave=record["wave"].as_u64()?;
    let dispatch=record["dispatch"].as_u64()?;
    let log=mustard_core::io::spec_events::read(&mustard_core::io::spec_events::spec_file(root,spec).ok()?).ok()??;
    let send=log.last_by_wave("send").get(&wave).copied()?;
    (log.dispatch_position(send)==dispatch).then(||(spec.into(),wave,dispatch))
}

pub(super) fn observe(root:&Path,host:&str,event:&str,input:&HookInput) {
    if !["SubagentStop","PostToolUse"].contains(&event) {return;}
    let Some(session)=input.session_id.as_deref().filter(|s|!s.is_empty()) else {return;};
    let agent=input.agent_id.as_deref().or_else(||input.raw.pointer("/tool_response/agent_id").or_else(||input.raw.pointer("/tool_response/agentId")).and_then(Value::as_str)).filter(|s|token(s));
    let Some(agent)=agent else {return;};
    let folder=directory(root);
    if !inside(root,&folder) {return;}
    let path=folder.join(format!("{host}-{}-{}.json",digest(session),digest(agent)));
    let Some(mut lock)=fs::lock::LockedFile::exclusive_if_free(&path).ok().flatten() else {return;};
    let mut record:Value=lock.read_to_string().ok().filter(|s|s.len()<=32_768).and_then(|s|serde_json::from_str(&s).ok()).unwrap_or_else(||json!({"host":host}));
    // Native tool completion provides exact request -> agent identity. The stop
    // event supplies its real file even when the host stores it elsewhere.
    let prompt=input.tool_input.get("prompt").or_else(||input.tool_input.get("message")).and_then(Value::as_str);
    let transcript=input.raw.get("agent_transcript_path").and_then(Value::as_str).filter(|s|s.len()<=4096);
    let initial=transcript.and_then(|p|opening(Path::new(p)));
    let text=prompt.or_else(||initial.as_ref().map(|p|p.0.as_str()));
    if let Some(identity)=text.and_then(|text|ReadDispatch::of(text).or_else(||heading(text).map(|(spec,wave)|ReadDispatch {spec,wave,root:mustard_core::io::wave_prompt::shown(root)}))) {
        if identity.root!=mustard_core::io::wave_prompt::shown(root) {return;}
        let Some(log)=mustard_core::io::spec_events::spec_file(root,&identity.spec).ok().and_then(|p|mustard_core::io::spec_events::read(&p).ok().flatten()) else {return;};
        let Some(send)=log.last_by_wave("send").get(&identity.wave).and_then(|id|log.get(*id)) else {return;};
        // The title alone is never enough. A native rewritten prompt must
        // contain the authoritative dispatch, not a similarly named task.
        if let Some(text)=text.filter(|text|ReadDispatch::of(text).is_none()) {
            let Some(expected)=send.str_field("text").filter(|s|!s.trim().is_empty()) else {return;};
            if !text.trim().strip_prefix(expected.trim()).is_some_and(|rest|rest.is_empty() || rest.starts_with('\n')) {return;}
        }
        if send.str_field("host").is_some_and(|h|h!=host) {return;}
        if let Some((_,stamp))=&initial {
            let time=chrono::DateTime::parse_from_rfc3339(stamp).ok();
            let sent=chrono::DateTime::parse_from_rfc3339(send.at()).ok();
            if time.zip(sent).is_none_or(|(at,since)|at<since) {return;}
        }
        record["spec"]=json!(identity.spec); record["wave"]=json!(identity.wave);
        record["dispatch"]=json!(log.dispatch_position(send.id));
    }
    if let Some(transcript)=transcript {record["path"]=json!(transcript);}
    if let Some(model)=input.raw.get("model").and_then(Value::as_str).filter(|s|token(s)) {record["model"]=json!(model);}
    record["at"]=json!(chrono::Utc::now().to_rfc3339());
    let _=lock.replace(record.to_string().as_bytes());
}

/// Only explicit records for this send. Unknown/missing format stays unknown;
/// partial histories are never presented as a complete wave total.
pub(crate) fn wave_usage(root:&Path,spec:&str,log:&SpecLog,wave:u64)->Option<Measured> {
    let folder=directory(root);
    if !inside(root,&folder) {return None;}
    let entries=std::fs::read_dir(folder).ok()?.collect::<Result<Vec<_>,_>>().ok()?;
    if entries.len()>4096 {return None;}
    let sends:std::collections::BTreeSet<_>=log.events.iter().filter(|e|e.event_type=="send" && e.wave()==Some(wave)).map(|e|log.dispatch_position(e.id)).collect();
    let mut found=std::collections::BTreeMap::<u64,Vec<Value>>::new();
    let all_records=entries.iter().filter_map(|entry|read(&entry.path())).collect::<Vec<_>>();
    for record in &all_records {
        if record["spec"]!=spec || record["wave"]!=wave {continue;}
        let dispatch=record["dispatch"].as_u64()?;
        if sends.contains(&dispatch) {found.entry(dispatch).or_default().push(record.clone());}
    }
    if sends.is_empty() || sends.iter().any(|send|!found.contains_key(send)) {return None;}
    let mut total=transcript::Usage::default();
    let mut claude_files=std::collections::BTreeSet::new();
    let mut codex_files=std::collections::BTreeSet::new();
    for records in found.values() {for record in records {
        let path=PathBuf::from(record["path"].as_str()?);
        if all_records.iter().any(|other|other["path"]==record["path"] && (other["spec"]!=spec || other["wave"]!=wave || other["dispatch"]!=record["dispatch"])) {return None;}
        match record["host"].as_str()? {
            "claude"=> {claude_files.insert(path);},
            "codex"=> {codex_files.insert(path); if let Some(model)=record["model"].as_str() {total.model=Some(model.into());}},
            _=>return None,
        }
    }}
    // Sharing a rollout across waves is not independently attributable.
    let has_codex=!codex_files.is_empty();
    let has_claude=!claude_files.is_empty();
    for path in codex_files {
        let value=super::codex_usage(&path)?;
        let input=value["input_tokens"].as_u64()?;
        let cached=value["cached_input_tokens"].as_u64()?;
        let output=value["output_tokens"].as_u64()?;
        total.tokens=total.tokens.checked_add(value["total_tokens"].as_u64()?)?;
        total.breakdown.input_tokens=total.breakdown.input_tokens.checked_add(input.checked_sub(cached)?)?;
        total.breakdown.cache_read_input_tokens=total.breakdown.cache_read_input_tokens.checked_add(cached)?;
        total.breakdown.output_tokens=total.breakdown.output_tokens.checked_add(output)?;
    }
    let texts=claude_files.iter().map(std::fs::read_to_string).collect::<Result<Vec<_>,_>>().ok()?;
    if !texts.is_empty() {
        let measured=transcript::usage_of(texts.iter().flat_map(|s|s.lines()));
        if measured.model.is_none() && measured.tokens==0 {return None;}
        total.model=measured.model; total.steps=measured.steps;
        total.tokens=total.tokens.checked_add(measured.tokens)?; total.breakdown.combine(&measured.breakdown);
    }
    if has_codex && has_claude {total.model=None;}
    let steps=(!has_codex).then_some(total.steps);
    (total.tokens>0 || total.model.is_some()).then_some(Measured {usage:total,steps})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_rewritten_claude_prompts_require_canonical_content_not_a_matching_title() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();
        std::fs::write(root.join("mustard.json"),"{}").unwrap();
        let path=mustard_core::io::spec_events::spec_file(root,"work").unwrap();
        let text=format!("{}\n\nCanonical bounded task.",mustard_core::domain::wave_prompt::wave_title("work",1,mustard_core::platform::i18n::Locale::PtBr));
        mustard_core::io::spec_events::write(&path,"send",json!({"wave":1,"role":"wave","lines":3,"chars":text.len(),"mustard":"test","host":"claude","text":text}).as_object().unwrap().clone(),&[]).unwrap();
        let log=mustard_core::io::spec_events::read(&path).unwrap().unwrap();
        let rollout=root.join("nonstandard-agent.jsonl");
        let save=|prompt:&str|std::fs::write(&rollout,format!("{}\n{}\n",json!({"type":"user","timestamp":chrono::Utc::now().to_rfc3339(),"message":{"role":"user","content":prompt}}),json!({"type":"assistant","message":{"id":"a1","model":"claude-sonnet-test","usage":{"input_tokens":10,"cache_read_input_tokens":20,"output_tokens":5}}}))).unwrap();
        let input=serde_json::from_value(json!({"cwd":root,"hook_event_name":"SubagentStop","session_id":"s","agent_id":"a","agent_transcript_path":rollout})).unwrap();
        save(&text.replace("Canonical bounded task.","A different task."));
        let _=crate::dispatch::run_event(Some(mustard_core::domain::model::contract::Trigger::SubagentStop),&input);
        assert!(wave_usage(root,"work",&log,1).is_none());
        save(&text);
        let _=crate::dispatch::run_event(Some(mustard_core::domain::model::contract::Trigger::SubagentStop),&input);
        assert_eq!(wave_usage(root,"work",&log,1).unwrap().usage.tokens,35);
    }
    #[test]
    fn native_stop_locates_nonstandard_rollout_and_never_reuses_it_for_a_resend() {
        let dir=tempfile::tempdir().unwrap(); let root=dir.path();
        std::fs::write(root.join("mustard.json"),"{}").unwrap();
        let path=mustard_core::io::spec_events::spec_file(root,"work").unwrap();
        let command=format!("mustard-rt run read request-1 --root {} --spec work",mustard_core::io::wave_prompt::shown(root));
        let written=mustard_core::io::spec_events::write(&path,"send",json!({"wave":1,"role":"wave","lines":1,"chars":command.len(),"mustard":"test","host":"codex","text":command}).as_object().unwrap().clone(),&[]).unwrap();
        let log=mustard_core::io::spec_events::read(&path).unwrap().unwrap();
        let transcript=root.join("native-thread.jsonl");
        std::fs::write(&transcript,format!("{}\n{}\n",json!({"timestamp":chrono::Utc::now().to_rfc3339(),"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":command}]}}),json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":20,"total_tokens":120}}}}))).unwrap();
        let input=serde_json::from_value(json!({"cwd":root,"hook_event_name":"SubagentStop","session_id":"s","agent_id":"agent","agent_transcript_path":transcript})).unwrap();
        crate::host::codex("SubagentStop",&input);
        let measured=wave_usage(root,"work",&log,1).unwrap();
        assert!(measured.steps.is_none());let usage=measured.usage;
        assert_eq!(usage.tokens,120); assert_eq!(usage.breakdown.input_tokens,20); assert_eq!(usage.breakdown.cache_read_input_tokens,80);
        let panel=crate::commands::flow::round::usage_snapshot(root,&log,None,None);
        assert_eq!(panel["waves"][0]["usage"]["tokens"],120);
        assert!(panel["waves"][0]["usage"].get("steps").is_none(),"the rollout does not expose tool steps");
        let second=directory(root).join("conflicting.json");
        let original=std::fs::read_dir(directory(root)).unwrap().find_map(|entry|read(&entry.ok()?.path())).unwrap();
        let mut conflicting=original.clone(); conflicting["wave"]=json!(2);
        std::fs::write(&second,conflicting.to_string()).unwrap();
        assert!(wave_usage(root,"work",&log,1).is_none(),"a shared rollout is not attributable to one wave");
        std::fs::remove_file(second).unwrap();
        let mut next=log.clone();
        let mut send=next.get(written.id).unwrap().clone(); send.id+=1; send.fields.insert("resends".into(),json!(written.id)); next.events.push(send);
        assert!(wave_usage(root,"work",&next,1).is_none());
        assert!(wave_usage(root,"other",&log,1).is_none());
    }
}
