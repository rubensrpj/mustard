//! Explicit native diagnostics of stored evidence and derived indexes.
//! Structural consistency does not certify live sources or business meaning.
use crate::domain::project_map::MapRefusal;
use crate::io::project_map::{self as store, unreadable};
use serde_json::{Value, json};
use std::path::Path;

pub fn run(root: &Path) -> Result<Value, MapRefusal> {
    let mut db = store::open_existing(&store::model_path(root))?;
    db.write(|tx| {
        let integrity: String=tx.query_row("PRAGMA quick_check",[],|r|r.get(0))?;
        for table in ["knowledge_fts","resource_fts","decl_fts","file_fts"] {
            tx.execute(&format!("INSERT INTO {table}({table}) VALUES ('integrity-check')"),[])?;
        }
        let checks=[
            ("invalid_analysis_json","SELECT count(*) FROM texts WHERE analysis IS NOT NULL AND analysis!='' AND NOT json_valid(analysis)"),
            ("missing_symbol_posting","SELECT count(*) FROM knowledge_symbols s LEFT JOIN knowledge_fts f ON f.rowid=s.rowid WHERE f.rowid IS NULL"),
            ("orphan_symbol_posting","SELECT count(*) FROM knowledge_fts f LEFT JOIN knowledge_symbols s ON s.rowid=f.rowid WHERE s.rowid IS NULL"),
            ("symbol_receipt_mismatch","SELECT count(*) FROM knowledge_symbols s LEFT JOIN texts t ON t.path=s.path WHERE t.path IS NULL OR coalesce(CASE WHEN json_valid(t.analysis) THEN json_extract(t.analysis,'$.knowledge.cards['||s.position||'].id') END,'')!=s.id OR coalesce(CASE WHEN json_valid(t.analysis) THEN json_extract(t.analysis,'$.knowledge.cards['||s.position||'].source.sha256') END,'')!=s.sha256"),
            ("missing_resource_posting","SELECT count(*) FROM resource_positions p LEFT JOIN resource_fts f ON f.rowid=p.id WHERE f.rowid IS NULL"),
            ("orphan_resource_posting","SELECT count(*) FROM resource_fts f LEFT JOIN resource_positions p ON p.id=f.rowid WHERE p.id IS NULL"),
            ("invalid_resource_position","SELECT count(*) FROM resource_positions p LEFT JOIN resource_files f ON f.path=p.path WHERE f.path IS NULL OR f.issue!='' OR CASE WHEN json_valid(f.sections) THEN json_extract(f.sections,'$['||p.section||']') END IS NULL"),
            ("resource_index_receipt_mismatch","SELECT count(*) FROM resource_index_files i LEFT JOIN resource_files f ON f.path=i.path WHERE f.path IS NULL OR f.issue!='' OR i.sha256!=f.sha256"),
            ("resource_source_not_indexed","SELECT count(*) FROM resource_files f LEFT JOIN resource_index_files i ON i.path=f.path WHERE f.issue='' AND i.path IS NULL"),
            ("resource_excerpt_count_mismatch","SELECT count(*) FROM resource_files f WHERE f.issue='' AND CASE WHEN json_valid(f.sections) THEN json_array_length(f.sections) END != (SELECT count(*) FROM resource_positions p WHERE p.path=f.path)"),
            ("dangling_static_link","SELECT count(*) FROM knowledge_links e LEFT JOIN knowledge_symbols a ON a.id=e.source LEFT JOIN knowledge_symbols b ON b.id=e.target WHERE a.id IS NULL OR b.id IS NULL"),
            ("document_reference_receipt_mismatch","SELECT count(*) FROM knowledge_resource_refs r LEFT JOIN resource_files f ON f.path=r.resource LEFT JOIN knowledge_symbols s ON s.id=r.target WHERE f.path IS NULL OR s.id IS NULL OR f.sha256!=r.resource_sha256 OR s.sha256!=r.target_sha256")
        ];
        let mut issues=Vec::new();
        let missing:i64=tx.query_row("SELECT count(*) FROM texts t,json_each(CASE WHEN json_valid(t.analysis) THEN json_extract(t.analysis,'$.knowledge.cards') ELSE '[]' END) c LEFT JOIN knowledge_symbols s ON s.path=t.path AND s.position=CAST(c.key AS INTEGER) WHERE CASE WHEN json_valid(t.analysis) THEN json_extract(t.analysis,'$.knowledge.version') END=?1 AND s.id IS NULL",
            [super::catalog::integer(crate::domain::knowledge::VERSION)?],|r|r.get(0))?;
        if missing>0 {issues.push(json!({"check":"canonical_symbol_missing","count":missing}));}
        for (name,sql) in checks {let count:i64=tx.query_row(sql,[],|r|r.get(0))?;if count>0 {issues.push(json!({"check":name,"count":count}));}}
        if integrity!="ok" {issues.push(json!({"check":"sqlite-quick-check","detail":integrity}));}
        let mut counts=serde_json::Map::new();
        for (key,sql) in [
            ("symbols","SELECT count(*) FROM knowledge_symbols"),
            ("static_links","SELECT count(*) FROM knowledge_links"),
            ("resource_files","SELECT count(*) FROM resource_files WHERE issue=''"),
            ("resource_exclusions","SELECT count(*) FROM resource_files WHERE issue!=''"),
            ("resource_excerpts","SELECT count(*) FROM resource_positions"),
            ("document_references","SELECT count(*) FROM knowledge_resource_refs"),
            ("unresolved_document_references","SELECT count(*) FROM knowledge_ref_issues"),
            ("interpretations","SELECT count(*) FROM knowledge_notes")
        ] {let n:i64=tx.query_row(sql,[],|r|r.get(0))?;counts.insert(key.into(),json!(n));}
        let mut plans=Vec::new();
        for sql in [
            "SELECT id FROM knowledge_symbols WHERE name='example' COLLATE NOCASE",
            "SELECT analysis FROM texts WHERE path='example'",
            "SELECT sections FROM resource_files WHERE path='example'",
            "SELECT source FROM knowledge_links WHERE target='example'",
            "SELECT resource FROM knowledge_resource_refs WHERE target='example'"
        ] {
            let mut statement=tx.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
            let details=statement.query_map([],|r|r.get::<_,String>(3))?.collect::<rusqlite::Result<Vec<_>>>()?;
            plans.push(json!({"query":sql,"plan":details}));
        }
        Ok(json!({"ok":issues.is_empty(),"question":"audit","sqlite":integrity,"fts_integrity":"ok","issues":issues,
            "counts":counts,"query_plans":plans,"scope":"stored evidence/index consistency; does not prove source freshness, semantic meaning or runtime behavior",
            "local_model_calls":0,"remote_model_calls":0}))
    }).map_err(unreadable)
}
