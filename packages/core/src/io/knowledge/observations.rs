//! Durable, verified source facts. Choices and inferred meaning are never facts.
use crate::domain::knowledge::resources::Registry;
use crate::io::map_db::{Block, Kind, MapDb};
use crate::io::project_map as store;
use crate::io::sha256::Sha256;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) const BLOCK:Block=Block {
    name:"search_facts",version:1,tables:&["search_files","search_facts"],
    schema:"CREATE TABLE IF NOT EXISTS search_files(tree TEXT NOT NULL,path TEXT NOT NULL,sha256 TEXT NOT NULL,indexed TEXT NOT NULL DEFAULT '',PRIMARY KEY(tree,path));
        CREATE TABLE IF NOT EXISTS search_facts(tree TEXT NOT NULL,path TEXT NOT NULL,line INTEGER NOT NULL,sha256 TEXT NOT NULL,text TEXT NOT NULL,PRIMARY KEY(tree,path,line));
        CREATE INDEX IF NOT EXISTS search_facts_source ON search_facts(tree,path,sha256);",
    kind:Kind::Written(convert),
};
fn convert(conn: &Connection, _version: u32) -> crate::platform::error::Result<()> {
    conn.execute_batch(BLOCK.schema)?;
    Ok(())
}
fn hash(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    digest.hex_digest()
}
fn tree_key(tree: &Path) -> String {
    hash(tree.to_string_lossy().as_bytes())
}

pub fn record(root: &Path, tree: &Path, hits: &[(String, u64, String)]) -> Result<Value, String> {
    if hits.is_empty() {
        let needs_scan=if store::model_path(root).is_file() {
            let db=MapDb::open_waiting(&store::model_path(root),root,&store::DB_BLOCKS,std::time::Duration::from_millis(50)).map_err(|e|e.to_string())?;
            !structure_ready(&db)?
        }else{false};
        return Ok(json!({"status":"no-source-facts","new_facts":0,"needs_scan":needs_scan,"source_hashes":{}}));
    }
    let registry = Registry::load()?;
    let mut sources = BTreeMap::new();
    let mut bytes = 0;
    for (file, _, _) in hits {
        if sources.contains_key(file) {
            continue;
        }
        if sources.len() >= 96 || bytes >= 12 * 1024 * 1024 {
            break;
        }
        if let Some(text) = super::investigation::safe_read(tree, file, &registry) {
            bytes += text.len();
            sources.insert(file.clone(), (hash(text.as_bytes()), text));
        }
    }
    // Remember source versions from the whole result, then distribute stored
    // line witnesses across files instead of spending them on an early file.
    let mut by_file=BTreeMap::<&str,Vec<_>>::new();
    for hit in hits {if sources.contains_key(&hit.0) {by_file.entry(&hit.0).or_default().push(hit);}}
    let mut witnesses=Vec::new();let mut at=0;
    while witnesses.len()<256 {
        let mut added=false;
        for group in by_file.values() {
            if witnesses.len()>=256 {break;}
            if let Some(hit)=group.get(at) {witnesses.push(*hit);added=true;}
        }
        if !added {break;}at+=1;
    }
    let mut db = MapDb::open_waiting(
        &store::model_path(root),
        root,
        &store::DB_BLOCKS,
        std::time::Duration::from_millis(50),
    )
    .map_err(|e| e.to_string())?;
    // A schema upgrade rebuilds derived blocks empty. A source hash learned
    // under the old schema cannot acknowledge that missing structural index.
    let structure_ready=structure_ready(&db)?;
    let key = tree_key(tree);
    let mut added = 0;
    let mut reused = 0;
    let mut pending = Vec::new();
    db.write(|tx| {
        for (file,(digest,_)) in &sources {
            let old:Option<String>=tx.query_row("SELECT sha256 FROM search_files WHERE tree=?1 AND path=?2",params![key,file],|row|row.get(0)).optional()?;
            if old.as_ref().is_some_and(|old|old!=digest) {tx.execute("DELETE FROM search_facts WHERE tree=?1 AND path=?2",params![key,file])?;}
            tx.execute("INSERT INTO search_files(tree,path,sha256) VALUES(?1,?2,?3) ON CONFLICT(tree,path) DO UPDATE SET sha256=excluded.sha256",params![key,file,digest])?;
            // A scanned document or a code file with zero declarations is
            // still a current snapshot. Requiring a symbol causes a complete
            // scan on the first hit in every such file, despite identical bytes.
            let indexed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_symbols WHERE path=?1 AND sha256=?2)
                OR EXISTS(SELECT 1 FROM resource_index_files WHERE path=?1 AND sha256=?2)
                OR EXISTS(SELECT 1 FROM texts WHERE path=?1 AND CASE WHEN json_valid(analysis) THEN json_extract(analysis,'$.content_sha256') END=?2)",params![file,digest],|row|row.get(0))?;
            let learned:bool=tx.query_row("SELECT indexed=sha256 FROM search_files WHERE tree=?1 AND path=?2",params![key,file],|row|row.get(0))?;
            if !structure_ready || !indexed && !learned {pending.push(file.clone());}
        }
        for (file,line,text) in &witnesses {
            let Some((digest,source))=sources.get(file) else {continue};
            if *line==0 || text.len()>4000 || source.lines().nth(line.saturating_sub(1) as usize)!=Some(text) {continue;}
            let Ok(line)=i64::try_from(*line) else{continue};
            let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM search_facts WHERE tree=?1 AND path=?2 AND line=?3 AND sha256=?4 AND text=?5)",params![key,file,line,digest,text],|row|row.get(0))?;
            if exists {reused+=1;continue;}
            tx.execute("INSERT INTO search_facts(tree,path,line,sha256,text) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(tree,path,line) DO UPDATE SET sha256=excluded.sha256,text=excluded.text",params![key,file,line,digest,text])?;
            added+=1;
        }
        Ok(())
    }).map_err(|e|e.to_string())?;
    Ok(
        json!({"status":"stored-current-source-facts","new_facts":added,"reused_facts":reused,"needs_scan":!pending.is_empty(),"pending_paths":pending,"omitted_line_witnesses":hits.len().saturating_sub(witnesses.len()),
        "tree":key,"source_hashes":sources.iter().map(|(file,(digest,_))|(file,digest)).collect::<BTreeMap<_,_>>(),"interpretations_recorded":0}),
    )
}

fn structure_ready(db:&MapDb)->Result<bool,String> {
    for block in [&store::CENSUS,&store::FILES,&store::DECLS,&store::GRAPH,&store::RESOURCES] {
        if db.mark(block.name()).map_err(|e|e.to_string())?.is_none_or(|mark|mark.is_empty()) {return Ok(false);}
    }
    Ok(true)
}

/// Mark only the source versions the completed native scan was asked to learn.
pub fn scanned(root: &Path, tree: &Path, learning: &Value) -> Result<(), String> {
    let mut db = super::open_existing(&store::model_path(root)).map_err(|e| format!("{e:?}"))?;
    db.write(|tx| {
        for (file, digest) in learning["source_hashes"].as_object().into_iter().flatten() {
            if let Some(digest) = digest.as_str() {
                tx.execute(
                    "UPDATE search_files SET indexed=?3 WHERE tree=?1 AND path=?2 AND sha256=?3",
                    params![tree_key(tree), file, digest],
                )?;
            }
        }
        Ok(())
    })
    .map_err(|e| e.to_string())
}
