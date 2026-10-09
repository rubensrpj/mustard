//! Host-acknowledged evidence delivery, scoped by checkout/agent/context epoch.
//! Native output and explicit Read never change. Only complete current bodies
//! acknowledged by the adapter may be replaced by an explicit reuse reference.
use super::{Answer, Request};
use crate::domain::code_search::contract::DeliveryContext;
use crate::domain::knowledge::{Source, resources::Registry};
use crate::io::{fs::lock::LockedFile, knowledge::investigation::safe_read, sha256::Sha256};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Default, Deserialize, Serialize)]
struct State {
    #[serde(default)]
    delivered: BTreeSet<String>,
    #[serde(default)]
    pending: BTreeMap<String, BTreeSet<String>>,
}
fn digest(bytes: &[u8]) -> String { let mut h=Sha256::new();h.update(bytes);h.hex_digest() }

/// Transport integrity check, not authentication. Host truncation or a hook
/// rewrite must not acknowledge source the model did not actually receive.
pub fn receipt(token:&str,bytes:&[u8])->String {
    let checksum=bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64,|hash,byte|(hash^u64::from(*byte)).wrapping_mul(0x0100_0000_01b3));
    format!("\n# mustard-delivery:{token}:{}:{checksum:016x}\n",bytes.len())
}

/// Best effort: a busy/corrupt/unavailable ledger returns the complete view.
/// Call only for an agent presentation; diagnostics and raw output stay whole.
pub fn prepare(answer: &mut Answer, request: &Request, tree: &Path, context: &DeliveryContext) -> Option<String> {
    context.validate().ok()?;
    if request.purpose == crate::domain::knowledge::investigation::Purpose::Locate || request.tool == "Read"
        || answer.report["task_context"]["status"] != "current-task-evidence" { return None; }
    let tree=tree.canonicalize().ok()?;
    let scope=digest(format!("{}\0{}\0{}\0{}",tree.display(),context.session,context.agent,context.epoch).as_bytes());
    let path=crate::ClaudePaths::for_project(&tree).ok()?.claude_dir().join(".session").join(&context.session).join(format!("delivery-{scope}.json"));
    let mut locked=LockedFile::exclusive_if_free(&path).ok()??;
    let previous=locked.read_to_string().ok()?;
    if previous.len()>2*1024*1024 { return None; }
    let mut state:State=if previous.is_empty(){State::default()}else{serde_json::from_str(&previous).ok()?};
    for token in &context.acknowledged {
        if let Some(keys)=state.pending.remove(token) {state.delivered.extend(keys);}
    }
    let registry=Registry::load().ok()?;
    let mut keys=BTreeSet::new();
    let mut reused=Vec::new();
    let cards=answer.report["task_context"]["cards"].as_array_mut()?;
    for (at,card) in cards.iter().enumerate().filter(|(_,c)|c["initial_source_excerpt"]==true && c["source_excerpt"]["truncated"]==false) {
        // Tiny bodies cost less than a reuse reference; deliver them normally.
        if card["source_excerpt"]["text"].as_str()?.len()<=super::task_view::reuse_notice(card)?.len() {continue;}
        let source:Source=serde_json::from_value(card["source"].clone()).ok()?;
        let text=safe_read(&tree,&source.file,&registry)?;
        if digest(text.as_bytes())!=source.sha256 { return None; }
        let key=digest(format!("{}\0{}\0{}\0{}\0{}",source.file,source.sha256,source.line,source.end_line,
            card["source_excerpt"]["text"].as_str()?).as_bytes());
        if state.delivered.contains(&key) {reused.push(at);} else {keys.insert(key);}
    }
    // Bound local bookkeeping; forgetting delivers source again, never hides it.
    if state.delivered.len()>4096 {state.delivered.clear();}
    if state.pending.len()>=128 {state.pending.clear();}
    let token=if keys.is_empty(){None}else{Some(digest(serde_json::to_string(&keys).ok()?.as_bytes()))};
    if let Some(token)=&token {state.pending.insert(token.clone(),keys);}
    if locked.replace(&serde_json::to_vec(&state).ok()?).is_err() {
        return None;
    }
    for at in &reused {cards[*at]["reused_delivery"]=json!(true);}
    answer.report["delivery"]=json!({"reused_complete_bodies":reused.len(),"acknowledgement_required":true,
        "scope":"checkout/session/agent/epoch/current-source-range","receipt":token});
    token
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::knowledge::investigation::Purpose;
    #[test]
    fn transport_receipt_counts_utf8_bytes_and_matches_the_host_checksum() {
        assert_eq!(receipt("token","ação 😀".as_bytes()),"\n# mustard-delivery:token:11:97a51403e9838ed2\n");
    }
    fn response(tree:&Path)->Answer {
        let source=std::fs::read_to_string(tree.join("a.rs")).unwrap();
        Answer {stdout:vec![],stderr:vec![],exit_code:0,report:json!({"task_context":{"status":"current-task-evidence","cards":[
            {"id":"a.rs:1:f","name":"f","source":{"file":"a.rs","line":1,"end_line":2,"sha256":digest(source.as_bytes())},
            "initial_source_excerpt":true,"source_excerpt":{"text":source,"truncated":false}}]}})}
    }
    #[test]
    fn only_acknowledged_same_context_current_complete_bodies_are_reused() {
        let dir=tempfile::tempdir().unwrap();let tree=dir.path();
        std::fs::write(tree.join("mustard.json"),"{}").unwrap();
        std::fs::write(tree.join("a.rs"),format!("fn f() {{\n{}}}\n","    let snapshot=1;\n".repeat(20))).unwrap();
        let request=Request{tool:"rg".into(),input:json!({"args":["f","."]}),intent:"inspect f".into(),purpose:Purpose::Implement,choose:false};
        let mut context=DeliveryContext{session:"test".into(),agent:"main".into(),epoch:"one".into(),acknowledged:vec![]};
        let token=prepare(&mut response(tree),&request,tree,&context).unwrap();
        let mut unacknowledged=response(tree);prepare(&mut unacknowledged,&request,tree,&context);
        assert_ne!(unacknowledged.report["task_context"]["cards"][0]["reused_delivery"],true);
        context.acknowledged.push(token);
        let mut repeated=response(tree);prepare(&mut repeated,&request,tree,&context);
        assert_eq!(repeated.report["task_context"]["cards"][0]["reused_delivery"],true);
        context.agent="worker".into();let mut other=response(tree);prepare(&mut other,&request,tree,&context);
        assert_ne!(other.report["task_context"]["cards"][0]["reused_delivery"],true);
        context.agent="main".into();context.epoch="compacted".into();let mut compacted=response(tree);prepare(&mut compacted,&request,tree,&context);
        assert_ne!(compacted.report["task_context"]["cards"][0]["reused_delivery"],true);
        context.epoch="one".into();std::fs::write(tree.join("a.rs"),"fn f() {changed();}\n").unwrap();
        let mut edited=response(tree);prepare(&mut edited,&request,tree,&context);
        assert_ne!(edited.report["task_context"]["cards"][0]["reused_delivery"],true);
        let mut read=response(tree);prepare(&mut read,&Request{tool:"Read".into(),..request},tree,&context);
        assert_ne!(read.report["task_context"]["cards"][0]["reused_delivery"],true);
    }
    #[test]
    fn tiny_and_incomplete_bodies_are_delivered_without_receipt_overhead() {
        let dir=tempfile::tempdir().unwrap();let tree=dir.path();
        std::fs::write(tree.join("mustard.json"),"{}").unwrap();std::fs::write(tree.join("a.rs"),"fn f() {}\n").unwrap();
        let request=Request{tool:"rg".into(),input:json!({"args":["f","."]}),intent:"inspect f".into(),purpose:crate::domain::knowledge::investigation::Purpose::Implement,choose:false};
        let context=DeliveryContext{session:"test".into(),agent:"main".into(),epoch:"one".into(),acknowledged:vec![]};
        assert!(prepare(&mut response(tree),&request,tree,&context).is_none());
        let mut incomplete=response(tree);incomplete.report["task_context"]["cards"][0]["source_excerpt"]["truncated"]=json!(true);
        assert!(prepare(&mut incomplete,&request,tree,&context).is_none());
    }
}
