//! Native resource retrieval. The source text is persisted once and only
//! candidate files are decoded; a contentless index never substitutes for it.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::domain::knowledge::Source;
use crate::domain::knowledge::resources::{Section, query_terms};
use crate::domain::normalize::{Languages, Normalizer};
use crate::domain::project_map::MapRefusal;
use crate::io::project_map::{self as store, unreadable};
use crate::platform::error::{Error, Result};

fn words(normalizer: &mut Normalizer, text: &str) -> String {
    normalizer.forms(text).into_iter().flatten().collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>().join(" ")
}

/// Called within the same scan transaction as the source packs. The index
/// can also be rebuilt locally when the query's normalization changes.
pub(crate) fn rebuild(conn: &Connection, languages: &Languages) -> Result<()> {
    conn.execute("DELETE FROM resource_fts", [])?;
    conn.execute("DELETE FROM resource_positions", [])?;
    conn.execute("DELETE FROM resource_meta", [])?;
    let mut normalizer = Normalizer::new(languages);
    let mut select = conn.prepare("SELECT path, sections FROM resource_files WHERE issue='' ORDER BY path")?;
    let rows = select.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut insert = conn.prepare("INSERT INTO resource_fts(rowid,title,body,path) VALUES (?1,?2,?3,?4)")?;
    let mut locate = conn.prepare("INSERT INTO resource_positions(id,path,section) VALUES (?1,?2,?3)")?;
    let mut id: i64 = 0;
    for row in rows {
        let (path, sections) = row?;
        let sections: Vec<Section> = serde_json::from_str(&sections).map_err(|err| Error::Parse(err.to_string()))?;
        for (n, section) in sections.iter().enumerate() {
            id += 1;
            insert.execute(params![id, words(&mut normalizer, &section.title), words(&mut normalizer, &section.text), words(&mut normalizer, &path)])?;
            let section = i64::try_from(n).map_err(|err| Error::Parse(err.to_string()))?;
            locate.execute(params![id, path, section])?;
        }
    }
    conn.execute("INSERT INTO resource_meta(key,value) VALUES ('languages',?1)", [languages.codes().join(",")])?;
    Ok(())
}

#[derive(Default)]
pub(super) struct Retrieved {
    pub items: Vec<Value>,
    pub stale: usize,
    pub omitted: usize,
    pub indexed_files: i64,
    pub issues: Vec<Value>,
    pub available: bool,
}

pub(super) fn retrieve(
    root: &Path,
    tree: &Path,
    opts: &super::Query<'_>,
    hashes: &mut BTreeMap<String, Option<(String, u64)>>,
) -> std::result::Result<Retrieved, MapRefusal> {
    let mut db = store::open_existing(&store::model_path(root))?;
    // An old map has an empty new block. Preserve readability, explicitly
    // reporting lack of coverage until a scan fills it.
    let mut result =
        Retrieved { available: db.mark(store::RESOURCES.name()).map_err(unreadable)?.is_some_and(|mark| !mark.is_empty()), ..Retrieved::default() };
    if !result.available {
        return Ok(result);
    }
    let languages = Languages::of_project(tree);
    let index_languages: Option<String> =
        db.conn().query_row("SELECT value FROM resource_meta WHERE key='languages'", [], |row| row.get(0)).optional().map_err(|err| unreadable(err.into()))?;
    if index_languages.as_deref() != Some(languages.codes().join(",").as_str()) {
        db.write(|tx| rebuild(tx, &languages)).map_err(unreadable)?;
    }
    result.indexed_files =
        db.conn().query_row("SELECT count(*) FROM resource_files WHERE issue=''", [], |row| row.get(0)).map_err(|err| unreadable(err.into()))?;
    let mut issues = db.conn().prepare("SELECT path,issue FROM resource_files WHERE issue!='' ORDER BY path LIMIT 20").map_err(|err| unreadable(err.into()))?;
    result.issues = issues
        .query_map([], |row| Ok(json!({"file":row.get::<_,String>(0)?,"reason":row.get::<_,String>(1)?})))
        .map_err(|err| unreadable(err.into()))?
        .collect::<std::result::Result<_, _>>()
        .map_err(|err| unreadable(err.into()))?;
    if opts.symbol.is_some() {
        return Ok(result);
    }
    let terms = query_terms(opts.text, &languages);
    let match_text = terms
        .iter()
        .flatten()
        .take(128)
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(" OR ");
    if !opts.text.trim().is_empty() && match_text.is_empty() {
        return Ok(result);
    }
    let sql = if match_text.is_empty() {
        "SELECT path,section FROM resource_positions WHERE (?2 IS NULL OR path=?2) AND ?1='' ORDER BY path,section"
    } else {
        "SELECT p.path,p.section FROM resource_fts JOIN resource_positions p ON p.id=resource_fts.rowid WHERE resource_fts MATCH ?1 AND (?2 IS NULL OR p.path=?2) ORDER BY bm25(resource_fts,4,1,0.25),p.id"
    };
    let mut select = db.conn().prepare(sql).map_err(|err| unreadable(err.into()))?;
    let rows =
        select.query_map(params![match_text, opts.file], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))).map_err(|err| unreadable(err.into()))?;
    let max = if opts.all { usize::MAX } else { opts.limit.clamp(1, 4) };
    let mut seen = BTreeSet::new();
    let mut cache = BTreeMap::<String, (String, String, Vec<Section>)>::new();
    // Inspect only a bounded candidate set for ordinary discovery. --all
    // remains an explicit full export of matching source excerpts.
    for (n, row) in rows.enumerate() {
        if !opts.all && n == 256 {
            result.omitted += 1;
            break;
        }
        let (path, at) = row.map_err(|err| unreadable(err.into()))?;
        let at = usize::try_from(at).map_err(|err| super::invalid(err.to_string()))?;
        if !opts.all && opts.file.is_none() && seen.contains(&path) {
            continue;
        }
        if result.items.len() == max {
            result.omitted += 1;
            break;
        }
        if !cache.contains_key(&path) {
            let (hash, kind, sections): (String, String, String) = db
                .conn()
                .query_row("SELECT sha256,kind,sections FROM resource_files WHERE path=?1", [&path], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .map_err(|err| unreadable(err.into()))?;
            cache.insert(path.clone(), (hash, kind, serde_json::from_str(&sections).map_err(|err| super::invalid(err.to_string()))?));
        }
        let Some((hash, kind, sections)) = cache.get(&path) else { continue };
        let Some(section) = sections.get(at) else { return Err(super::invalid("knowledge-resource-position-invalid")) };
        let source = Source { file: path.clone(), line: section.line, end_line: section.end_line, sha256: hash.clone() };
        if !super::current(tree, &source, hashes) {
            result.stale += 1;
            seen.insert(path);
            continue;
        }
        let mut normalizer = Normalizer::new(&languages);
        let own: BTreeSet<_> = normalizer.forms(&format!("{} {}", section.title, section.text)).into_iter().flatten().collect();
        let hits = terms.iter().filter(|slot| slot.iter().any(|term| own.contains(term))).count();
        let exact_path = opts.text.trim() == path || Path::new(&path).file_name().and_then(|name| name.to_str()) == Some(opts.text.trim());
        // Resource prose is supplemental evidence: require every informative
        // query slot, rather than injecting matches on two generic words.
        if !exact_path && !terms.is_empty() && hits < terms.len() {
            continue;
        }
        let (excerpt, excerpt_line, compact) = if opts.detail || opts.all {
            (section.text.clone(), section.line, false)
        } else {
            crate::domain::knowledge::resources::preview(section, &terms, &languages)
        };
        result.items.push(json!({"id":format!("resource:{path}:{}",section.line),"title":section.title,"kind":kind,
            "source":source,"text":excerpt,"excerpt_line":excerpt_line,"text_compacted":compact,"text_chars":section.text.chars().count(),
            "retrieval":"native-resource-text-index","extraction":"verbatim-line-excerpt","semantic_proof":false,
            "behavior_validation":"unknown; text is an author assertion or configuration/schema source, not effective runtime behavior"}));
        seen.insert(path);
    }
    Ok(result)
}

pub(super) fn for_source(root: &Path, tree: &Path, file: &str, name: &str) -> std::result::Result<Value, MapRefusal> {
    let query = super::Query {
        text: name,
        file: Some(file),
        limit: 2,
        depth: 0,
        all: false,
        detail: false,
        symbol: None,
        direction: super::Direction::Outgoing,
        refresh: false,
    };
    let report = retrieve(root, tree, &query, &mut BTreeMap::new())?;
    if report.items.is_empty() {
        return Ok(Value::Null);
    }
    let mut version = crate::io::sha256::Sha256::new();
    version.update(json!(report.items).to_string().as_bytes());
    Ok(json!({"resources":report.items,"evidence_version":version.hex_digest(),"semantic_proof":false,
        "expand":"run knowledge --file <source file> --detail"}))
}
