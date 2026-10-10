//! Document-to-code addresses are maintained separately from call edges.
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::domain::knowledge::resources::Section;
use crate::domain::knowledge::{Card, Source, references};
use crate::domain::project_map::MapRefusal;
use crate::io::project_map::{self as store, unreadable};
use crate::platform::error::{Error, Result};

/// Catalogue changes can change uniqueness even when a document is intact.
/// Re-resolve explicit addresses in the same transaction as both sources.
pub(crate) fn rebuild(conn: &Connection) -> Result<()> {
    conn.execute_batch("DELETE FROM knowledge_resource_refs; DELETE FROM knowledge_ref_issues;")?;
    let mut select = conn.prepare("SELECT path,sha256,sections FROM resource_files WHERE issue='' AND kind='documentation' ORDER BY path")?;
    let rows = select.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
    for row in rows {
        let (path, hash, text) = row?;
        let sections: Vec<Section> = serde_json::from_str(&text).map_err(|e| Error::Parse(e.to_string()))?;
        let mut text = String::new();
        let mut next_line = 1;
        for section in &sections {
            for _ in next_line..section.line {
                text.push('\n');
            }
            text.push_str(&section.text);
            next_line = section.end_line + 1;
        }
        for reference in references::extract(&text) {
            let Some(section) = sections.iter().find(|s| s.line <= reference.line && reference.line <= s.end_line) else { continue };
            let targets = resolve(conn, &path, &reference.text)?;
            if targets.is_empty() {
                conn.execute("INSERT INTO knowledge_ref_issues(resource,section,ref_line,reference,reason,candidates) VALUES (?1,?2,?3,?4,'unresolved-explicit-reference',0)",
                    params![path,super::catalog::integer(section.line)?,super::catalog::integer(reference.line)?,reference.text])?;
                continue;
            }
            let symbol_address = references::address(&path, &reference.text).is_none();
            if symbol_address && targets.len() != 1 {
                conn.execute(
                    "INSERT INTO knowledge_ref_issues(resource,section,ref_line,reference,reason,candidates) VALUES (?1,?2,?3,?4,'ambiguous-symbol',?5)",
                    params![path, super::catalog::integer(section.line)?, super::catalog::integer(reference.line)?, reference.text, targets.len() as i64],
                )?;
                continue;
            }
            for (target, target_hash) in targets {
                conn.execute("INSERT OR IGNORE INTO knowledge_resource_refs(resource,section,ref_line,reference,target,kind,resource_sha256,target_sha256) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![path,super::catalog::integer(section.line)?,super::catalog::integer(reference.line)?,reference.text,target,
                        if symbol_address {"explicit-unique-symbol"} else {"explicit-file-address"},hash,target_hash])?;
            }
        }
    }
    Ok(())
}

fn resolve(conn: &Connection, document: &str, text: &str) -> Result<Vec<(String, String)>> {
    if let Some((relative, line)) = references::address(document, text) {
        let mut select = conn.prepare(
            "SELECT id,sha256,(end_line-line) FROM knowledge_symbols WHERE path=?1 AND (?2 IS NULL OR (line<=?2 AND end_line>=?2)) ORDER BY (end_line-line),line,id",
        )?;
        let line = line.map(super::catalog::integer).transpose()?;
        let mut rows = select
            .query_map(params![relative, line], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // A root-relative quoted path is a second explicit interpretation,
        // accepted only when the document-relative address found no source.
        if rows.is_empty()
            && !text.starts_with('.')
            && let Some((root, _)) = references::address("index", text)
        {
            rows = select.query_map(params![root, line], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        }
        if line.is_some() {
            let width = rows.first().map(|row| row.2);
            rows.retain(|row| Some(row.2) == width);
        }
        return Ok(rows.into_iter().map(|(id, hash, _)| (id, hash)).collect());
    }
    // Addresses with unsupported syntax never fall back to a guessed name.
    if text.contains(['/', '#', ':', '\\']) || text.chars().any(|c| !c.is_alphanumeric() && c != '_') {
        return Ok(vec![]);
    }
    let mut select = conn.prepare("SELECT id,sha256 FROM knowledge_symbols WHERE name=?1 ORDER BY path,line")?;
    Ok(select.query_map([text], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(super) fn targets(root: &Path, items: &[Value]) -> std::result::Result<BTreeMap<String, Source>, MapRefusal> {
    let db = store::open_existing(&store::model_path(root))?;
    let mut select=db.conn().prepare("SELECT r.target,s.path,s.line,s.end_line,r.target_sha256 FROM knowledge_resource_refs r JOIN knowledge_symbols s ON s.id=r.target WHERE r.resource=?1 AND r.section=?2 AND r.resource_sha256=?3 AND r.target_sha256=s.sha256 ORDER BY s.path,s.line")
        .map_err(|e|unreadable(e.into()))?;
    let mut out = BTreeMap::new();
    for item in items {
        let Some(source) = item.get("source").and_then(|s| serde_json::from_value::<Source>(s.clone()).ok()) else { continue };
        let rows = select
            .query_map(params![source.file, super::catalog::integer(source.line).map_err(unreadable)?, source.sha256], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    Source { file: r.get(1)?, line: r.get::<_, i64>(2)? as u64, end_line: r.get::<_, i64>(3)? as u64, sha256: r.get(4)? },
                ))
            })
            .map_err(|e| unreadable(e.into()))?;
        for row in rows {
            let (id, source) = row.map_err(|e| unreadable(e.into()))?;
            out.insert(id, source);
        }
    }
    Ok(out)
}

pub(super) fn linked_documents(
    root: &Path,
    tree: &Path,
    cards: &[Card],
    hashes: &mut BTreeMap<String, Option<(String, u64)>>,
    detail: bool,
) -> std::result::Result<Vec<Value>, MapRefusal> {
    let db = store::open_existing(&store::model_path(root))?;
    let mut select=db.conn().prepare("SELECT r.resource,r.section,r.ref_line,r.reference,r.kind,r.resource_sha256,f.sections FROM knowledge_resource_refs r JOIN resource_files f ON f.path=r.resource WHERE r.target=?1 AND r.target_sha256=?2 AND r.resource_sha256=f.sha256 AND f.issue='' ORDER BY r.resource,r.section")
        .map_err(|e|unreadable(e.into()))?;
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for card in cards {
        if !super::current(tree, &card.source, hashes) {
            continue;
        }
        let rows = select
            .query_map(params![card.id, card.source.sha256], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })
            .map_err(|e| unreadable(e.into()))?;
        for row in rows {
            let (file, line, at, reference, kind, sha256, sections) = row.map_err(|e| unreadable(e.into()))?;
            if !seen.insert((file.clone(), line)) {
                continue;
            }
            let sections: Vec<Section> = serde_json::from_str(&sections).map_err(|e| super::invalid(e.to_string()))?;
            let Some(section) = sections.iter().find(|s| s.line == line as u64) else { continue };
            let source = Source { file, line: section.line, end_line: section.end_line, sha256 };
            if !super::current(tree, &source, hashes) {
                continue;
            }
            let (text, excerpt_line, compacted) = if detail {
                (section.text.clone(), section.line, false)
            } else {
                let languages = crate::domain::normalize::Languages::of_project(tree);
                crate::domain::knowledge::resources::preview(section, &crate::domain::knowledge::resources::query_terms(&card.name, &languages), &languages)
            };
            out.push(json!({"id":format!("resource:{}:{}",source.file,source.line),"kind":"documentation","title":section.title,"source":source,
                "text":text,"excerpt_line":excerpt_line,"text_compacted":compacted,"semantic_proof":false,"retrieval":"explicit-document-reference",
                "reference":{"target":card.id,"line":at,"spelling":reference,"kind":kind,"meaning":"author reference; not a call or validated behavioral claim"}}));
            if !detail && out.len() >= 2 {
                return Ok(out);
            }
        }
    }
    Ok(out)
}
