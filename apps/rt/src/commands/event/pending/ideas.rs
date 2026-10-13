//! Ideas share the pending ledger, lock and IDs. They are suggestions, never
//! approved tasks. Linking to an existing spec neither creates nor runs waves.
use super::{json,Ledger,Value,item_json,Path,ledger_root,load,open_locked,today,one_line,Status,text,push_open,Owner,write};

fn revision(ledger:&Ledger)->u64 {ledger.revision}
fn view(ledger:&Ledger)->Value {
    json!({"ok":true,"schema_version":1,"revision":revision(ledger),"items":ledger.items.iter().map(|item|{
        let mut shown=item_json(item);shown["idea"]=json!(item.idea);shown
    }).collect::<Vec<_>>()})
}
pub(crate) fn run(start:&Path,patch:Option<&str>)->Value {
    let canonical=mustard_core::io::spec_events::spec_root(start);
    let root=if mustard_core::ProjectConfig::exists(&canonical){ledger_root(&canonical)}else{canonical};
    let path=root.join(".claude/pending/ledger.json");
    let Some(raw)=patch else {return load(&path).map_or_else(|error|error,|ledger|view(&ledger));};
    if !mustard_core::ProjectConfig::exists(&root) {return json!({"ok":false,"reason":"project-not-initialized"});}
    let result=(||->Result<Value,Value>{
        let invalid=||json!({"ok":false,"reason":"idea-invalid-request"});
        if raw.len()>16384 {return Err(invalid());}
        let request:Value=serde_json::from_str(raw).map_err(|_|invalid())?;
        if request["schema_version"]!=1 || request.as_object().is_none_or(|object|object.keys().any(|key|!["schema_version","revision","action","id","title","detail","spec","reason"].contains(&key.as_str()))) {return Err(invalid());}
        let (mut file,mut ledger)=open_locked(&path)?;
        if request["revision"]!=revision(&ledger) {return Err(json!({"ok":false,"reason":"idea-conflict"}));}
        let today=today(None);
        let action=request["action"].as_str().ok_or_else(invalid)?;
        let title=request["title"].as_str().map(one_line).unwrap_or_default();
        let detail=request["detail"].as_str().map(one_line).unwrap_or_default();
        if matches!(action,"add"|"edit") && (title.is_empty() || detail.is_empty() || title.len()>300 || detail.len()>10000) {return Err(invalid());}
        if matches!(action,"add"|"edit") && let Some(existing)=ledger.items.iter().find(|item|item.status==Status::Open && text::fold(&item.title)==text::fold(&title) && request["id"]!=item.id) {
            return Err(json!({"ok":false,"reason":"duplicate","id":existing.id}));
        }
        let id=if action=="add" {
            let id=push_open(&mut ledger,title.clone(),detail.clone(),None,&today);
            if let Some(item)=ledger.items.last_mut() {item.idea=true;item.owner=Owner::Project;}
            id
        }else {request["id"].as_str().ok_or_else(invalid)?.to_string()};
        let item=ledger.items.iter_mut().find(|item|item.id==id).ok_or_else(||json!({"ok":false,"reason":"idea-not-found"}))?;
        match action {
            "add"=>{},
            "edit" if item.idea && item.status==Status::Open=>{item.title=title;item.detail=detail;},
            "archive"|"complete" if item.status==Status::Open=>{
                let reason=request["reason"].as_str().filter(|reason|!reason.trim().is_empty()).ok_or_else(invalid)?;
                item.status=if action=="archive"{Status::Dropped}else{Status::Closed};item.reason=Some(one_line(reason));
            },
            "reopen" if item.status!=Status::Open=>{item.status=Status::Open;item.reason=None;},
            "link" if item.status==Status::Open=>{
                let spec=request["spec"].as_str().ok_or_else(invalid)?;
                let exists=mustard_core::io::spec_events::spec_file(&root,spec).is_ok_and(|path|path.is_file());
                if !exists {return Err(json!({"ok":false,"reason":"idea-spec-not-found"}));}
                item.became=Some(spec.into());item.status=Status::Closed;item.reason=Some(format!("Spec: {spec}"));
            },
            _=>return Err(invalid()),
        }
        write(&mut file,&mut ledger,&today)?;
        let mut response=view(&ledger);response["id"]=json!(id);Ok(response)
    })();
    result.unwrap_or_else(|error|error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{load,count_line_of,Locale,add_for_the_project};
    #[test]
    fn suggestions_share_ids_but_do_not_become_obligations_and_stale_edits_fail() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
        let added=run(root,Some(&json!({"schema_version":1,"revision":0,"action":"add","title":"Download","detail":"Melhorar Excel"}).to_string()));
        assert_eq!(added["ok"],true);assert_eq!(added["id"],"P-1");
        let ledger=load(&root.join(".claude/pending/ledger.json")).unwrap();
        assert!(count_line_of(&ledger,"2026-10-13",Locale::default()).is_none());
        assert_eq!(add_for_the_project(root,"Outra tarefa","Realizar" ).unwrap(),"P-2");
        let stale=run(root,Some(&json!({"schema_version":1,"revision":1,"action":"edit","id":"P-1","title":"Novo","detail":"Texto"}).to_string()));
        assert_eq!(stale["reason"],"idea-conflict");
        assert_eq!(run(root,None)["items"].as_array().unwrap().len(),2);
        assert!(!root.join(".claude/spec").exists());
    }
}
