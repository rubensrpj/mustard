//! Complementary native discovery; never rewrites the caller's original tool.
use super::{Request,scope::Scope};
use crate::domain::knowledge::investigation::{Matcher,Purpose};
use crate::domain::normalize::Languages;
use crate::io::knowledge::{self,investigation};
use serde_json::{Value,json};
use std::collections::BTreeSet;
use std::path::Path;

pub(super) fn probe(root:&Path,tree:&Path,request:&Request,scope:&Scope,seeds:&mut Vec<String>)->Value {
    let names=crate::domain::code_search::pattern_names(&scope.clues);
    if names.len()==1 && seeds.iter().any(|id|id.rsplit(':').next().is_some_and(|name|names.iter().any(|asked|name.eq_ignore_ascii_case(asked)))) {
        return json!({"status":"exact-name; native discovery sufficient","remote_model_calls":0});
    }
    let Ok((clues,paths))=knowledge::catalog::discovery_probe(root,&request.intent,&scope.clues,&scope.files) else {
        return json!({"status":"catalog-unavailable; original retained","remote_model_calls":0});
    };
    if clues.is_empty() || paths.is_empty(){return json!({"status":"no-additional-written-clues","remote_model_calls":0});}
    let Ok(registry)=crate::domain::knowledge::resources::Registry::load() else {return Value::Null};
    let mut admitted=Vec::new();let mut bytes=0;let mut skipped=0;
    for path in &paths {
        if admitted.len()>=96 || bytes>=12*1024*1024 {break;}
        if let Some(text)=investigation::safe_read(tree,path,&registry) {
            if bytes+text.len()>12*1024*1024 {skipped+=1;continue;}
            bytes+=text.len();admitted.push(path.clone());
        }else{skipped+=1;}
    }
    if admitted.is_empty(){return json!({"status":"no-admissible-source","remote_model_calls":0});}
    let mut args=vec!["-n".to_string(),"--with-filename".into(),"--no-heading".into(),"--color=never".into(),"-i".into(),"-F".into()];
    for clue in &clues {args.extend(["-e".into(),clue.clone()]);}
    args.push("--".into());args.extend(admitted.iter().cloned());
    let native=Request{tool:"rg".into(),input:json!({"args":args}),intent:String::new(),purpose:Purpose::Locate,choose:false};
    let Ok(answer)=super::execute_native(tree,&native) else {return json!({"status":"native-probe-unavailable; original retained","remote_model_calls":0});};
    if answer.exit_code>1{return json!({"status":"native-probe-failed; original retained","remote_model_calls":0});}
    let Ok(mut matcher)=Matcher::new(&request.intent,&Languages::of_project(root)) else {return Value::Null};
    // Prefixes are only a native prefilter. Full normalized query slots must
    // match current source before a line can influence owners or learning.
    let hits:Vec<_>=super::occurrences(tree,tree,&native,&answer.report["result"],&answer.stdout).into_iter().filter(|(_,_,text)|!matcher.matched(text).is_empty()).collect();
    let borrowed:Vec<_>=hits.iter().map(|(file,line,text)|investigation::Occurrence{file,line:*line,text}).collect();
    let before:BTreeSet<_>=seeds.iter().cloned().collect();
    if let Ok(crossed)=investigation::cross_hits(root,tree,&borrowed) {
        for id in crossed["current_owner_ids"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if !seeds.iter().any(|seed|seed==id){seeds.push(id.into());}
        }
    }
    let learning=knowledge::observations::record(root,tree,&hits).ok();
    json!({"status":"native-question-probe","method":"native area + co-occurring rare question clues -> scoped rg -> full normalized source match -> current owners",
        "files":admitted.len(),"bytes_admitted":bytes,"candidate_files":paths.len(),"skipped_or_unavailable":skipped,
        "partial":admitted.len()<paths.len(),"verified_occurrences":hits.len(),"added_owners":seeds.iter().filter(|id|!before.contains(*id)).count(),
        "added_owner_ids":seeds.iter().filter(|id|!before.contains(*id)).collect::<Vec<_>>(),
        "learning":learning,"original_query_rewritten":false,"remote_model_calls":0})
}
