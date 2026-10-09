//! Cross every admitted source occurrence before selecting reading evidence.
//! Load each source and its innermost owner intervals once; native order does
//! not decide which lines of an already admitted file can be crossed.
use super::{Occurrence,MAX_LIVE_FILES,MAX_LIVE_BYTES,LIVE_TIME,hash,safe_read,invalid};
use crate::domain::knowledge::resources::Registry;
use crate::domain::project_map::MapRefusal;
use std::collections::{BTreeMap,BTreeSet};
use std::path::Path;
use std::time::Instant;

pub(super) struct Crossed {
    pub files:BTreeMap<String,(String,String)>,
    pub ids:BTreeSet<String>,
    pub matched:BTreeMap<String,Vec<u64>>,
    pub unmatched:usize,
    pub processed:usize,
}
pub(super) fn collect(conn:&rusqlite::Connection,tree:&Path,hits:&[Occurrence<'_>],registry:&Registry,rich:bool)->Result<Crossed,MapRefusal> {
    let mut result=Crossed{files:BTreeMap::new(),ids:BTreeSet::new(),matched:BTreeMap::new(),unmatched:0,processed:0};
    let mut groups=BTreeMap::<&str,Vec<&Occurrence<'_>>>::new();
    for hit in hits {groups.entry(hit.file).or_default().push(hit);}
    let start=Instant::now();let mut bytes=0;
    for (file,hits) in groups {
        if start.elapsed()>=LIVE_TIME || result.files.len()>=MAX_LIVE_FILES || bytes>=MAX_LIVE_BYTES {break;}
        let Some(text)=safe_read(tree,file,registry) else {
            if !rich {return Err(invalid("knowledge-live-source-unavailable"));}
            result.unmatched+=hits.len();result.processed+=hits.len();continue;
        };
        bytes+=text.len();let digest=hash(text.as_bytes());
        let mut statement=conn.prepare("SELECT id,line,end_line FROM knowledge_symbols WHERE path=?1 AND sha256=?2 ORDER BY end_line-line,line DESC,id")
            .map_err(|e|super::super::unreadable(e.into()))?;
        let owners=statement.query_map(rusqlite::params![file,digest],|row|Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?,row.get::<_,i64>(2)?)))
            .map_err(|e|super::super::unreadable(e.into()))?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e|super::super::unreadable(e.into()))?;
        let lines:Vec<_>=text.lines().collect();
        for (at,hit) in hits.iter().enumerate() {
            if at%256==0 && start.elapsed()>=LIVE_TIME {break;}
            let line=i64::try_from(hit.line).map_err(|_|invalid("knowledge-occurrence-line-invalid"))?;
            let verified=hit.line>0 && usize::try_from(hit.line-1).ok().and_then(|line|lines.get(line)).copied()==Some(hit.text);
            if !verified {
                if !rich {return Err(invalid("knowledge-executed-hit-changed"));}
                result.unmatched+=1;
            } else if let Some((id,_,_))=owners.iter().find(|(_,first,last)|*first<=line && line<=*last) {
                result.ids.insert(id.clone());
                result.matched.entry(id.clone()).or_default().push(hit.line);
            } else {result.unmatched+=1;}
            result.processed+=1;
        }
        result.files.insert(file.into(),(digest,text));
    }
    for lines in result.matched.values_mut() {lines.sort_unstable();lines.dedup();}
    Ok(result)
}
