//! Reuse persisted declaration membership/contracts for targeted follow-ups.
//! Parser associations remain candidates and never establish runtime dispatch.
use super::{catalog, current, open_existing};
use crate::domain::knowledge::{Card, Source};
use rusqlite::params;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub struct Related {
    pub card: Card,
    pub receipt: Value,
}

pub struct FollowUp {
    pub targets: Vec<Related>,
    pub partial: bool,
}

pub fn follow(root:&Path,tree:&Path,files:&BTreeSet<String>,seeds:&[Card],limit:usize)->Result<FollowUp,String> {
    let db=open_existing(&super::store::model_path(root)).map_err(|e|format!("{e:?}"))?;
    let mut links=db.conn().prepare("SELECT members,implements,implemented_by FROM decls WHERE file=?1 AND line=?2 AND name=?3 LIMIT 2").map_err(|e|e.to_string())?;
    let mut candidates=Vec::<(String,Value)>::new();
    let mut hashes=BTreeMap::new();
    let mut seen=BTreeSet::new();
    let mut partial=false;
    for seed in seeds.iter().take(4).filter(|seed|files.contains(&seed.source.file)) {
        if !current(tree,&seed.source,&mut hashes) {continue;}
        let rows=links.query_map(params![seed.source.file,sql_line(seed.source.line)?,seed.name],|row|Ok([
            row.get::<_,Option<String>>(0)?.unwrap_or_else(||"[]".into()),
            row.get::<_,Option<String>>(1)?.unwrap_or_else(||"[]".into()),
            row.get::<_,Option<String>>(2)?.unwrap_or_else(||"[]".into())
        ])).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        partial|=rows.len()>1;
        if let [row]=rows.as_slice() {
            for (kind,text) in ["member","implements","implementation"].into_iter().zip(row) {
                let ids:Vec<String>=serde_json::from_str(text).map_err(|e|format!("invalid-stored-relation: {e}"))?;
                for id in ids {
                    let Some((file,_))=id.rsplit_once(':').and_then(|(head,_)|head.rsplit_once(':')) else {continue;};
                    if !files.contains(file) {partial=true;continue;}
                    if seen.insert((seed.id.clone(),id.clone(),kind)) {
                        candidates.push((id,json!({"from":seed.id,"relation":kind,"evidence_source":seed.source,
                            "resolution":"parser-declaration-association","meaning":"written owner/contract association; overloads and runtime dispatch unverified"})));
                    }
                }
            }
        }
        // Current caller receipts identify the innermost owning declaration.
        // Tied ranges are left unresolved instead of choosing a homonym.
        for edge in seed.callers.iter().filter(|edge|edge["resolution"]=="unique-static-target") {
            let Ok(source)=serde_json::from_value::<Source>(edge["source"].clone()) else {continue;};
            if !files.contains(&source.file) {partial=true;continue;}
            if !current(tree,&source,&mut hashes) {continue;}
            let owners=db.conn().prepare("SELECT id,line,end_line FROM knowledge_symbols WHERE path=?1 AND sha256=?2 AND line<=?3 AND end_line>=?3 ORDER BY end_line-line,line DESC LIMIT 2")
                .map_err(|e|e.to_string())?.query_map(params![source.file,source.sha256,sql_line(source.line)?],|row|Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?,row.get::<_,i64>(2)?)))
                .map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
            let Some((id,start,end))=owners.first() else {continue;};
            if owners.get(1).is_some_and(|(_,other_start,other_end)|other_end-other_start==end-start) {partial=true;continue;}
            if seen.insert((seed.id.clone(),id.clone(),"caller")) {candidates.push((id.clone(),json!({"from":seed.id,"relation":"caller",
                "evidence_source":source,"resolution":"unique-static-target","meaning":"current written call site; execution unverified"})));}
        }
    }
    // Bound hydration independently of presentation. No source outside the
    // caller's original inventory is read to fill a missing relationship.
    partial|=candidates.len()>128;
    candidates.truncate(128);
    let ids=candidates.iter().map(|(id,_)|id.clone()).collect();
    let cards:BTreeMap<_,_>=catalog::hydrate(db.conn(),&ids).map_err(|e|e.to_string())?.into_iter().map(|card|(card.id.clone(),card)).collect();
    let mut targets=Vec::new();
    for (id,mut receipt) in candidates {
        let Some(card)=cards.get(&id).filter(|card|files.contains(&card.source.file) && current(tree,&card.source,&mut hashes)) else {partial=true;continue;};
        receipt["to"]=json!(card.id);receipt["target_source"]=json!(card.source);
        targets.push(Related{card:card.clone(),receipt});
    }
    partial|=targets.len()>limit;
    targets.truncate(limit);
    Ok(FollowUp{targets,partial})
}

fn sql_line(line:u64)->Result<i64,String> {i64::try_from(line).map_err(|_|"source-line-overflow".into())}

