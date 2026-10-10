//! Optional, local code retrieval over actual AST ranges. Vectors choose
//! destinations; current source receipts still decide what may be returned.
//! No HTTP, service, query-time writes, or dependency on lexical hydration.
use std::{collections::{BTreeMap, BTreeSet}, path::Path, sync::OnceLock};
use model2vec_rs::model::StaticModel;
use rusqlite::{Connection, params};
use serde_json::Value;
use crate::{domain::knowledge::Card, io::{map_db::{Block, Kind, MapDb, table_exists}, sha256::Sha256}, platform::error::{Error, Result}};

const TOKENIZER: &[u8] = include_bytes!("../../assets/code-meaning/tokenizer.json");
const WEIGHTS: &[u8] = include_bytes!("../../assets/code-meaning/model.safetensors");
const CONFIG: &[u8] = include_bytes!("../../assets/code-meaning/config.json");
// Pinned encoder + document recipe + output quantization. Never mix encoders.
const MODEL: &str = "potion-code-16M-v2:e9d2a44ca6a05ac6685f3b23709ea57eb7352d5b:ast-v1:i8";
const BLOCK: Block = Block {name:"code-meaning",version:1,tables:&["code_vectors","code_vector_files"],
    schema:"CREATE TABLE code_vectors(id TEXT NOT NULL,chunk INTEGER NOT NULL,path TEXT NOT NULL,sha256 TEXT NOT NULL,vector BLOB NOT NULL,PRIMARY KEY(id,chunk)) WITHOUT ROWID; CREATE INDEX code_vectors_path ON code_vectors(path); CREATE TABLE code_vector_files(path TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,model TEXT NOT NULL) WITHOUT ROWID;",kind:Kind::Rebuilt(empty)};
#[allow(clippy::unnecessary_wraps)]
fn empty(_: &Connection, _: &Path) -> Result<()> {Ok(())}
fn model()->Option<&'static StaticModel> {
    static ENCODER:OnceLock<Option<StaticModel>>=OnceLock::new();
    ENCODER.get_or_init(||StaticModel::from_bytes(TOKENIZER,WEIGHTS,CONFIG,None).ok()).as_ref()
}
fn hash(bytes:&[u8])->String {let mut digest=Sha256::new();digest.update(bytes);digest.hex_digest()}
fn blob(vector:&[f32])->Vec<u8> {vector.iter().map(|v|((v*127.0).round().clamp(-127.0,127.0) as i8).to_le_bytes()[0]).collect()}
fn cosine(a:&[u8],b:&[u8])->f64 {
    if a.len()!=b.len(){return 0.0;}
    let (mut dot,mut x,mut y)=(0i32,0i32,0i32);
    for (a,b) in a.iter().zip(b) {let (a,b)=(i32::from(i8::from_le_bytes([*a])),i32::from(i8::from_le_bytes([*b])));dot+=a*b;x+=a*a;y+=b*b;}
    if x==0||y==0 {0.0}else{f64::from(dot)/(f64::from(x)*f64::from(y)).sqrt()}
}

/// Changed files only; hashes bind vectors to scanned bytes and metadata.
/// `--native` never calls this. Disabled configuration returns before opening DB.
pub fn fill_at(path:&Path,root:&Path)->Result<usize> {
    if !crate::domain::config::ProjectConfig::load(root).ai_vectors_enabled(){return Ok(0);}
    let mut db=MapDb::open(path,root,&[BLOCK])?;
    if !table_exists(db.conn(),"texts")? {return Ok(0);}
    let known:BTreeMap<String,String>={let mut q=db.conn().prepare("SELECT path,fingerprint FROM code_vector_files WHERE model=?1")?;
        q.query_map([MODEL],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?};
    let files:Vec<(String,String)>={let mut q=db.conn().prepare("SELECT path,analysis FROM texts WHERE analysis IS NOT NULL ORDER BY path")?;
        q.query_map([],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?};
    let present:BTreeSet<_>=files.iter().map(|(path,_)|path.clone()).collect();
    let registry=crate::domain::knowledge::resources::Registry::load().map_err(Error::config)?;
    let mut changes=Vec::new();let mut count=0;
    for (file,analysis) in files {
        let fingerprint=hash(analysis.as_bytes());
        if known.get(&file)==Some(&fingerprint) {continue;}
        let value:Value=serde_json::from_str(&analysis).map_err(|e|Error::Parse(e.to_string()))?;
        let mut documents=Vec::new();
        if let Some(text)=crate::io::knowledge::investigation::safe_read(root,&file,&registry) {
            let sha=hash(text.as_bytes());
            for item in value["knowledge"]["cards"].as_array().into_iter().flatten() {
                let card:Card=serde_json::from_value(item.clone()).map_err(|e|Error::Parse(e.to_string()))?;
                if sha!=card.source.sha256 {continue;}
                for (at,body) in documents_for(&card,&text).into_iter().enumerate() {documents.push((card.id.clone(),at,sha.clone(),body));}
            }
        }
        let vectors=if documents.is_empty(){Vec::new()}else{
            let encoder=model().ok_or_else(||Error::config("embedded code encoder is invalid"))?;
            documents.chunks(256).flat_map(|batch|encoder.encode_with_args(&batch.iter().map(|(_,_,_,text)|text.clone()).collect::<Vec<_>>(),None,256)).map(|v|blob(&v)).collect::<Vec<_>>()
        };
        count+=vectors.len();changes.push((file,fingerprint,documents,vectors));
    }
    if changes.is_empty() && known.keys().all(|p|present.contains(p)) {return Ok(0);}
    db.write(|tx| {
        for path in known.keys().filter(|p|!present.contains(*p)) {tx.execute("DELETE FROM code_vectors WHERE path=?1",[path])?;tx.execute("DELETE FROM code_vector_files WHERE path=?1",[path])?;}
        for (path,fingerprint,documents,vectors) in &changes {
            tx.execute("DELETE FROM code_vectors WHERE path=?1",[path])?;
            for ((id,at,sha,_),vector) in documents.iter().zip(vectors) {tx.execute("INSERT INTO code_vectors(id,chunk,path,sha256,vector) VALUES(?1,?2,?3,?4,?5)",params![id,*at as i64,path,sha,vector])?;}
            tx.execute("INSERT OR REPLACE INTO code_vector_files(path,fingerprint,model) VALUES(?1,?2,?3)",params![path,fingerprint,MODEL])?;
        }
        Ok(())
    })?;Ok(count)
}

fn documents_for(card:&Card,text:&str)->Vec<String> {
    let mut bodies=Vec::new();
    for chunk in card.syntax["source_chunks"].as_array().into_iter().flatten() {
        if let Some(body)=chunk["start_byte"].as_u64().zip(chunk["end_byte"].as_u64()).and_then(|(start,end)|text.get(start as usize..end as usize)) {bodies.push(body.to_string());}
    }
    if bodies.is_empty() {bodies.push(text.lines().skip(card.source.line.saturating_sub(1) as usize).take((card.source.end_line.saturating_sub(card.source.line)+1) as usize).collect::<Vec<_>>().join("\n"));}
    bodies.into_iter().map(|body|format!("{}\n{}\n{}\n{}\n{}",card.source.file,card.name,card.signature,card.documentation.chars().take(400).collect::<String>(),body)).collect()
}

/// Full scoped index before lexical cuts. Group chunks by symbol *before*
/// taking the budget, so a large class cannot consume all destinations.
#[derive(Default)]
pub(crate) struct Ranked {pub ids:Vec<String>,pub calls:u64}
pub(crate) fn ranked(conn:&Connection,question:&str,files:&BTreeSet<String>,limit:usize)->Result<Ranked> {
    if question.trim().is_empty() || !table_exists(conn,"code_vectors")? {return Ok(Ranked::default());}
    let inventory=serde_json::to_string(files).map_err(|e|Error::Parse(e.to_string()))?;
    let mut q=conn.prepare("SELECT v.id,v.vector FROM code_vectors v JOIN code_vector_files f ON f.path=v.path AND f.model=?1 JOIN knowledge_symbols s ON s.id=v.id AND s.sha256=v.sha256 WHERE v.path IN (SELECT value FROM json_each(?2))")?;
    let available:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM code_vectors v JOIN code_vector_files f ON f.path=v.path AND f.model=?1 JOIN knowledge_symbols s ON s.id=v.id AND s.sha256=v.sha256 WHERE v.path IN (SELECT value FROM json_each(?2)))",params![MODEL,inventory],|r|r.get(0))?;
    if !available{return Ok(Ranked::default());}
    let Some(encoder)=model() else{return Ok(Ranked::default())};
    let query=blob(&encoder.encode_with_args(&[question.to_string()],None,1)[0]);
    let mut rows=q.query(params![MODEL,inventory])?;let mut scores=BTreeMap::<String,f64>::new();
    while let Some(row)=rows.next()? {let id:String=row.get(0)?;let vector:Vec<u8>=row.get(1)?;let score=cosine(&query,&vector);if score>0.0 {scores.entry(id).and_modify(|s|*s=s.max(score)).or_insert(score);}}
    let mut order:Vec<_>=scores.into_iter().collect();order.sort_by(|(a,x),(b,y)|y.total_cmp(x).then(a.cmp(b)));
    Ok(Ranked{ids:order.into_iter().take(limit).map(|(id,_)|id).collect(),calls:1})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encoder_and_quantized_vectors_preserve_distinct_code_meanings() {
        let encoder=model().expect("valid pinned model");
        let vectors=encoder.encode_with_args(&["read and parse a JSON file".into(),"fn read_json(path) { serde_json::from_str(read_to_string(path)) }".into(),"fn sort_numbers(xs) { xs.sort() }".into()],None,3);
        let q=blob(&vectors[0]);assert_eq!(q.len(),256);
        assert!(cosine(&q,&blob(&vectors[1]))>cosine(&q,&blob(&vectors[2])));
    }
    #[test]
    fn optional_cache_refreshes_changed_bytes_removes_deleted_files_and_rejects_old_receipts() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();let path=root.join("grain.db");
        assert_eq!(fill_at(&path,root).unwrap(),0);assert!(!path.exists());
        std::fs::write(root.join("mustard.json"),r#"{"ai":{"vectors":true}}"#).unwrap();
        let db=MapDb::open(&path,root,&[]).unwrap();
        db.conn().execute_batch("CREATE TABLE texts(path TEXT PRIMARY KEY,analysis TEXT); CREATE TABLE knowledge_symbols(id TEXT PRIMARY KEY,sha256 TEXT);").unwrap();
        let write=|text:&str| {
            std::fs::write(root.join("main.rs"),text).unwrap();
            let sha=hash(text.as_bytes());
            let card=serde_json::json!({"id":"main.rs:1:read_json","name":"read_json","kind":"function","source":{"file":"main.rs","line":1,"end_line":1,"sha256":sha}});
            let value=serde_json::json!({"knowledge":{"cards":[card]}}).to_string();
            db.conn().execute("INSERT OR REPLACE INTO texts VALUES('main.rs',?1)",[value]).unwrap();
            db.conn().execute("INSERT OR REPLACE INTO knowledge_symbols VALUES('main.rs:1:read_json',?1)",[sha]).unwrap();
        };
        write("fn read_json() { parse_json(read_file()); }");
        assert_eq!(fill_at(&path,root).unwrap(),1);assert_eq!(fill_at(&path,root).unwrap(),0);
        let files=BTreeSet::from(["main.rs".to_string()]);
        assert_eq!(ranked(db.conn(),"read JSON from a file",&files,10).unwrap().ids,["main.rs:1:read_json"]);
        assert_eq!(ranked(db.conn(),"read JSON from a file",&BTreeSet::new(),10).unwrap().calls,0);
        write("fn read_json() { parse_json(read_other_file()); }");
        assert!(ranked(db.conn(),"read JSON from a file",&files,10).unwrap().ids.is_empty());
        assert_eq!(fill_at(&path,root).unwrap(),1);assert_eq!(fill_at(&path,root).unwrap(),0);
        db.conn().execute("DELETE FROM texts",[]).unwrap();fill_at(&path,root).unwrap();
        assert_eq!(db.conn().query_row("SELECT count(*) FROM code_vectors",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
}
