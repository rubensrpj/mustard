//! Reuse persisted declaration membership/contracts for targeted follow-ups.
//! Parser associations remain candidates; only an imported compiler index may
//! label a relationship precise. Neither establishes runtime dispatch.
use super::{catalog, current, open_existing};
use crate::domain::knowledge::{Card, Source};
use crate::domain::knowledge::precise::{Location, PreciseSymbols, Relation};
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
    let precise_available:bool=db.conn().prepare("SELECT path FROM precise_documents").ok().and_then(|mut statement|
        statement.query_map([],|row|row.get::<_,String>(0)).ok()?.collect::<Result<Vec<_>,_>>().ok())
        .is_some_and(|paths|!paths.is_empty() && paths.iter().all(|path|files.contains(path)));
    for seed in seeds.iter().take(4).filter(|seed|files.contains(&seed.source.file)) {
        if !current(tree,&seed.source,&mut hashes) {continue;}
        let rows=links.query_map(params![seed.source.file,sql_line(seed.source.line)?,seed.name],|row|Ok([
            row.get::<_,Option<String>>(0)?.unwrap_or_else(||"[]".into()),
            row.get::<_,Option<String>>(1)?.unwrap_or_else(||"[]".into()),
            row.get::<_,Option<String>>(2)?.unwrap_or_else(||"[]".into())
        ])).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        let mut has_contract=false;
        partial|=rows.len()>1;
        if let [row]=rows.as_slice() {
            for (kind,text) in ["member","implements","implementation"].into_iter().zip(row) {
                let ids:Vec<String>=serde_json::from_str(text).map_err(|e|format!("invalid-stored-relation: {e}"))?;
                has_contract|=kind=="implementation" && !ids.is_empty();
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
        if precise_available && (seed.kind=="interface" || has_contract)
            && let Some((line,column))=name_location(tree,seed)
            && let Ok(resolution)=(super::precise::ScipSymbols{root,tree}).resolve(&Location{file:&seed.source.file,line,column_bytes:column},Relation::Implementations,limit)
            && resolution.status=="current-index"
        {
            partial|=resolution.has_more;
            for reference in resolution.references {
                if !files.contains(&reference.source.file) {partial=true;continue;}
                let owners=db.conn().prepare("SELECT id,line,end_line FROM knowledge_symbols WHERE path=?1 AND sha256=?2 AND line<=?3 AND end_line>=?3 ORDER BY end_line-line,line DESC LIMIT 2")
                    .map_err(|e|e.to_string())?.query_map(params![reference.source.file,reference.source.sha256,sql_line(reference.source.line)?],|row|Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?,row.get::<_,i64>(2)?)))
                    .map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
                let Some((id,start,end))=owners.first() else {partial=true;continue;};
                if owners.get(1).is_some_and(|(_,other_start,other_end)|other_end-other_start==end-start) {partial=true;continue;}
                if seen.insert((seed.id.clone(),id.clone(),"compiler-implementation")) {
                    candidates.push((id.clone(),json!({"from":seed.id,"relation":"implementation","evidence_source":seed.source,
                        "resolution":"current-compiler-index","producer":resolution.producer,"index_sha256":resolution.index_sha256,
                        "meaning":"source-bound compiler relationship; runtime dispatch unverified"})));
                }
            }
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

fn name_location(tree:&Path,card:&Card)->Option<(u64,u64)> {
    if card.name.is_empty() {return None;}
    let bytes=super::source_bytes(tree,&card.source.file)?;
    let text=std::str::from_utf8(&bytes).ok()?;
    // Ambiguous lexical coordinates are deferred to an explicit References
    // request. This avoids querying an attribute or comment with the same name.
    let part=|c:char|c.is_alphanumeric() || matches!(c,'_'|'$');
    let locations:Vec<_>=text.lines().enumerate().skip(card.source.line.saturating_sub(1) as usize)
        .take((card.source.end_line-card.source.line+1).min(12) as usize)
        .flat_map(|(at,line)|line.match_indices(&card.name).filter(|(column,_)|
            line[..*column].chars().next_back().is_none_or(|c|!part(c))
            && line[*column+card.name.len()..].chars().next().is_none_or(|c|!part(c)))
            .map(move |(column,_)|(at as u64+1,column as u64))).collect();
    if let [location]=locations.as_slice() {Some(*location)} else {None}
}
