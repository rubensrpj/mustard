//! Derived symbol catalogue. Candidate selection precedes JSON hydration;
//! original evidence stays in texts.analysis and is never duplicated here.
use std::collections::BTreeSet;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::{Interpretation, Query, note_card};
use crate::domain::knowledge::resources::query_terms;
use crate::domain::knowledge::{self, Card};
use crate::domain::normalize::{Languages, Normalizer};
use crate::domain::project_map::MapRefusal;
use crate::io::map_db::{Block, Kind};
use crate::io::map_search::Discovery;
use crate::io::project_map::{self as store, unreadable};
use crate::io::sha256::Sha256;
use crate::platform::error::{Error, Result};

pub(crate) const BLOCK: Block = Block {
    name: "knowledge_index",
    version: 2,
    tables: &["knowledge_fts", "knowledge_links", "knowledge_symbols", "knowledge_files", "knowledge_meta", "knowledge_resource_refs", "knowledge_ref_issues"],
    schema: "CREATE TABLE knowledge_symbols(id TEXT PRIMARY KEY,path TEXT NOT NULL,name TEXT NOT NULL,line INTEGER NOT NULL,end_line INTEGER NOT NULL,sha256 TEXT NOT NULL,position INTEGER NOT NULL);\
        CREATE INDEX knowledge_by_path ON knowledge_symbols(path,line);\
        CREATE INDEX knowledge_by_name ON knowledge_symbols(name COLLATE NOCASE);\
        CREATE TABLE knowledge_links(source TEXT NOT NULL,target TEXT NOT NULL,PRIMARY KEY(source,target));\
        CREATE INDEX knowledge_by_target ON knowledge_links(target,source);\
        CREATE TABLE knowledge_files(path TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,evidence_version INTEGER NOT NULL);\
        CREATE TABLE knowledge_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);\
        CREATE VIRTUAL TABLE knowledge_fts USING fts5(name,own,path,header,intent,content='',contentless_delete=1,tokenize='unicode61 remove_diacritics 2');\
        CREATE TABLE knowledge_resource_refs(resource TEXT NOT NULL,section INTEGER NOT NULL,ref_line INTEGER NOT NULL,reference TEXT NOT NULL,target TEXT NOT NULL,kind TEXT NOT NULL,resource_sha256 TEXT NOT NULL,target_sha256 TEXT NOT NULL,PRIMARY KEY(resource,section,ref_line,target,kind));\
        CREATE INDEX knowledge_refs_target ON knowledge_resource_refs(target,resource);\
        CREATE TABLE knowledge_ref_issues(resource TEXT NOT NULL,section INTEGER NOT NULL,ref_line INTEGER NOT NULL,reference TEXT NOT NULL,reason TEXT NOT NULL,candidates INTEGER NOT NULL);",
    kind: Kind::Rebuilt(initialize),
};

fn initialize(conn: &Connection, root: &Path) -> Result<()> {
    sync(conn, &Languages::of_project(root))?;
    super::references::rebuild(conn)
}

fn words(normalizer: &mut Normalizer, text: &str) -> String {
    normalizer.forms(text).into_iter().flatten().collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>().join(" ")
}

/// Runs in the source transaction. Unchanged files retain their index rows.
pub(crate) fn sync(conn: &Connection, languages: &Languages) -> Result<()> {
    conn.execute_batch("CREATE INDEX IF NOT EXISTS knowledge_texts_path ON texts(path);")?;
    let codes = languages.codes().join(",");
    let stored: Option<String> = conn.query_row("SELECT value FROM knowledge_meta WHERE key='languages'", [], |r| r.get(0)).optional()?;
    let version: Option<String> = conn.query_row("SELECT value FROM knowledge_meta WHERE key='evidence_version'", [], |r| r.get(0)).optional()?;
    if stored.as_deref() != Some(&codes) || version.as_deref() != Some(&knowledge::VERSION.to_string()) {
        conn.execute_batch("DELETE FROM knowledge_fts; DELETE FROM knowledge_links; DELETE FROM knowledge_symbols; DELETE FROM knowledge_files;")?;
    }
    let mut normalizer = Normalizer::new(languages);
    let mut select = conn.prepare("SELECT path,analysis FROM texts ORDER BY path")?;
    let rows = select.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?;
    let mut present = BTreeSet::new();
    for row in rows {
        let (path, analysis) = row?;
        present.insert(path.clone());
        let text = analysis.unwrap_or_default();
        let mut digest = Sha256::new();
        digest.update(text.as_bytes());
        let fingerprint = digest.hex_digest();
        let old: Option<String> = conn.query_row("SELECT fingerprint FROM knowledge_files WHERE path=?1", [&path], |r| r.get(0)).optional()?;
        if old.as_deref() == Some(&fingerprint) {
            continue;
        }
        remove(conn, &path)?;
        let analysis: Value = if text.is_empty() { Value::Null } else { serde_json::from_str(&text).map_err(|e| Error::Parse(e.to_string()))? };
        if analysis["knowledge"]["version"] == knowledge::VERSION {
            for (position, value) in analysis["knowledge"]["cards"].as_array().into_iter().flatten().enumerate() {
                let card: Card = serde_json::from_value(value.clone()).map_err(|e| Error::Parse(e.to_string()))?;
                conn.execute(
                    "INSERT INTO knowledge_symbols(id,path,name,line,end_line,sha256,position) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![card.id, path, card.name, integer(card.source.line)?, integer(card.source.end_line)?, card.source.sha256, position as i64],
                )?;
                let rowid = conn.last_insert_rowid();
                let own = knowledge::evidence::own_text(&card);
                conn.execute(
                    "INSERT INTO knowledge_fts(rowid,name,own,path,header,intent) VALUES (?1,?2,?3,?4,?5,?6)",
                    params![
                        rowid,
                        words(&mut normalizer, &card.name),
                        words(&mut normalizer, &own),
                        words(&mut normalizer, &path),
                        words(&mut normalizer, &card.file_documentation),
                        words(
                            &mut normalizer,
                            &format!(
                                "{} {} {}",
                                card.documentation,
                                card.body_comment,
                                card.annotations.iter().map(|a| a.text.as_str()).collect::<Vec<_>>().join(" ")
                            )
                        )
                    ],
                )?;
                for edge in &card.outgoing {
                    if edge["resolution"] == "unique-static-target"
                        && let Some(target) = edge["target"].as_str()
                    {
                        conn.execute("INSERT OR IGNORE INTO knowledge_links(source,target) VALUES (?1,?2)", params![card.id, target])?;
                    }
                }
            }
        }
        conn.execute(
            "INSERT INTO knowledge_files(path,fingerprint,evidence_version) VALUES (?1,?2,?3)",
            params![path, fingerprint, integer(analysis["knowledge"]["version"].as_u64().unwrap_or(0))?],
        )?;
    }
    let mut select = conn.prepare("SELECT path FROM knowledge_files")?;
    let old = select.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for path in old {
        if !present.contains(&path) {
            remove(conn, &path)?;
        }
    }
    conn.execute("INSERT OR REPLACE INTO knowledge_meta(key,value) VALUES ('languages',?1)", [codes])?;
    conn.execute("INSERT OR REPLACE INTO knowledge_meta(key,value) VALUES ('evidence_version',?1)", [knowledge::VERSION.to_string()])?;
    Ok(())
}

fn remove(conn: &Connection, path: &str) -> Result<()> {
    conn.execute("DELETE FROM knowledge_fts WHERE rowid IN (SELECT rowid FROM knowledge_symbols WHERE path=?1)", [path])?;
    conn.execute("DELETE FROM knowledge_links WHERE source IN (SELECT id FROM knowledge_symbols WHERE path=?1)", [path])?;
    conn.execute("DELETE FROM knowledge_symbols WHERE path=?1", [path])?;
    conn.execute("DELETE FROM knowledge_files WHERE path=?1", [path])?;
    Ok(())
}

pub(super) struct Pool {
    pub cards: Vec<Card>,
    pub total: usize,
    pub omitted: bool,
    pub outdated: i64,
}

/// Apply the native search's file inventory before hydration. Broad words in
/// another directory must not consume a scoped task's candidate reservoir.
pub(super) fn scoped_candidates(root: &Path, options: &Query<'_>, scope: &super::EvidenceScope<'_>) -> std::result::Result<Pool, MapRefusal> {
    ensure_languages(root)?;
    let db = store::open_existing(&store::model_path(root))?;
    let conn = db.conn();
    let inventory = serde_json::to_string(scope.files).map_err(|e| super::invalid(e.to_string()))?;
    let total: i64 = conn.query_row("SELECT count(*) FROM knowledge_symbols WHERE path IN (SELECT value FROM json_each(?1))", [&inventory], |r| r.get(0)).map_err(|e| unreadable(e.into()))?;
    let mut ids = BTreeSet::new();
    let mut omitted = false;
    if let Some(id) = options.symbol {
        add_ids(conn, "SELECT id FROM knowledge_symbols WHERE id=?1 AND path IN (SELECT value FROM json_each(?2))", params![id, inventory], &mut ids, 512, &mut omitted)?;
    } else {
        let terms = query_terms(options.text, &Languages::of_project(root));
        for join in ["AND", "OR"] {
            let expression = fts_query(&terms, join);
            if !expression.is_empty() {
                add_ids(conn,
                    "SELECT s.id FROM knowledge_fts JOIN knowledge_symbols s ON s.rowid=knowledge_fts.rowid WHERE knowledge_fts MATCH ?1 AND s.path IN (SELECT value FROM json_each(?2)) ORDER BY bm25(knowledge_fts,5,2,0.25,1,0),s.path,s.line",
                    params![expression, inventory], &mut ids, 512, &mut omitted)?;
            }
        }
    }
    // Source owners are a reservoir, not a prefix allowed to consume the
    // whole hydration budget before the question has contributed candidates.
    for id in scope.seeds {
        add_ids(conn, "SELECT id FROM knowledge_symbols WHERE id=?1 AND path IN (SELECT value FROM json_each(?2))", params![id, inventory], &mut ids, 512, &mut omitted)?;
    }
    let cards = hydrate(conn, &ids).map_err(unreadable)?;
    Ok(Pool { cards, total: usize::try_from(total).unwrap_or(0), omitted, outdated: 0 })
}

pub(super) fn unindexed_files(root: &Path, scope: &super::EvidenceScope<'_>) -> std::result::Result<Vec<String>, MapRefusal> {
    let db=store::open_existing(&store::model_path(root))?;
    let inventory=serde_json::to_string(scope.files).map_err(|e|super::invalid(e.to_string()))?;
    let mut statement=db.conn().prepare("SELECT value FROM json_each(?1) WHERE NOT EXISTS(SELECT 1 FROM knowledge_symbols s WHERE s.path=value) ORDER BY value").map_err(|e|unreadable(e.into()))?;
    statement.query_map([inventory],|row|row.get(0)).map_err(|e|unreadable(e.into()))?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e|unreadable(e.into()))
}

pub(super) fn intent_weights(root: &Path, query: &str, languages: &Languages) -> std::result::Result<Vec<f64>, MapRefusal> {
    corpus_weights(root,query,languages,false)
}

pub(super) fn task_weights(root:&Path,query:&str,languages:&Languages)->std::result::Result<Vec<f64>,MapRefusal> {
    corpus_weights(root,query,languages,true)
}

fn corpus_weights(root:&Path,query:&str,languages:&Languages,symbols:bool)->std::result::Result<Vec<f64>,MapRefusal> {
    let db = store::open_existing(&store::model_path(root))?;
    let conn = db.conn();
    let count=if symbols{"count(*)"}else{"count(DISTINCT path)"};
    let files:i64=conn.query_row(&format!("SELECT {count} FROM knowledge_symbols"),[],|r|r.get(0)).map_err(|e|unreadable(e.into()))?;
    query_terms(query, languages)
        .into_iter()
        .map(|slot| {
            let expression = format!("{{name intent own path header}} : {}", fts_query(&[slot], "OR"));
            let seen: i64 = conn
                .query_row(
                    &format!("SELECT {} FROM knowledge_fts JOIN knowledge_symbols s ON s.rowid=knowledge_fts.rowid WHERE knowledge_fts MATCH ?1",if symbols{"count(*)"}else{"count(DISTINCT s.path)"}),
                    [expression],
                    |r| r.get(0),
                )
                .map_err(|e| unreadable(e.into()))?;
            Ok(((files + 1) as f64 / (seen + 1) as f64).ln() + 1.0)
        })
        .collect()
}

pub(super) fn exact_name(root: &Path, options: &Query<'_>) -> std::result::Result<bool, MapRefusal> {
    if options.text.trim().is_empty() || options.text.trim().chars().any(char::is_whitespace) || options.all {
        return Ok(false);
    }
    let db = store::open_existing(&store::model_path(root))?;
    Ok(db
        .conn()
        .query_row(
            "SELECT 1 FROM knowledge_symbols WHERE name=?1 COLLATE NOCASE AND (?2 IS NULL OR path=?2) LIMIT 1",
            params![options.text.trim(), options.file],
            |r| r.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| unreadable(e.into()))?
        .is_some())
}

pub(super) fn ensure_languages(root:&Path)->std::result::Result<(),MapRefusal> {
    let mut db=store::open_existing(&store::model_path(root))?;
    let languages=Languages::of_project(root);
    let codes = languages.codes().join(",");
    let stored: Option<String> =
        db.conn().query_row("SELECT value FROM knowledge_meta WHERE key='languages'", [], |r| r.get(0)).optional().map_err(|e| unreadable(e.into()))?;
    if stored.as_deref() != Some(&codes) {
        db.write(|tx| {
            sync(tx, &languages)?;
            super::references::rebuild(tx)?;
            crate::io::map_revision::bump(tx)
        })
        .map_err(unreadable)?;
    }
    Ok(())
}

/// A query hydrates matching cards, not every analysis object in the bank.
pub(super) fn candidates(
    root: &Path,
    options: &Query<'_>,
    notes: &[Interpretation],
    discovery: Option<&Discovery>,
    resource_targets: &[String],
    exact_name: bool,
) -> std::result::Result<Pool, MapRefusal> {
    ensure_languages(root)?;
    let db = store::open_existing(&store::model_path(root))?;
    let languages = Languages::of_project(root);
    let conn = db.conn();
    let total: i64 = conn.query_row("SELECT count(*) FROM knowledge_symbols", [], |r| r.get(0)).map_err(|e| unreadable(e.into()))?;
    let mut ids = BTreeSet::new();
    let mut omitted = false;
    let budget = if options.all { usize::MAX } else { 512 };
    if let Some(symbol) = options.symbol {
        ids.insert(symbol.to_string());
    } else {
        let exact = "SELECT id FROM knowledge_symbols WHERE name=?1 COLLATE NOCASE AND (?2 IS NULL OR path=?2) ORDER BY path,line";
        add_ids(conn, exact, params![options.text.trim(), options.file], &mut ids, budget, &mut omitted)?;
        if !exact_name {
            for target in resource_targets {
                ids.insert(target.clone());
            }
            for note in notes.iter().filter(|note| knowledge::interpretation_matches(&note_card(note), options.text, &languages)) {
                for source in &note.sources {
                    add_ids(
                        conn,
                        "SELECT id FROM knowledge_symbols WHERE path=?1 AND line<=?2 AND end_line>=?3 ORDER BY line",
                        params![source.file, integer(source.end_line).map_err(unreadable)?, integer(source.line).map_err(unreadable)?],
                        &mut ids,
                        usize::MAX,
                        &mut omitted,
                    )?;
                }
            }
            // Full informative matches precede the broad discovery reservoir;
            // otherwise common words can consume its hydration budget first.
            let terms = query_terms(options.text, &languages);
            let complete = fts_query(&terms, "AND");
            if !complete.is_empty() {
                add_ids(
                    conn,
                    "SELECT s.id FROM knowledge_fts JOIN knowledge_symbols s ON s.rowid=knowledge_fts.rowid WHERE knowledge_fts MATCH ?1 AND (?2 IS NULL OR s.path=?2) ORDER BY bm25(knowledge_fts,5,2,0.25,1,0),s.path,s.line",
                    params![complete, options.file],
                    &mut ids,
                    budget,
                    &mut omitted,
                )?;
            }
            let mut discovered=BTreeSet::new();
            let mut discovery_order=Vec::new();
            if let Some(discovery) = discovery {
                for (path, line, name) in &discovery.places {
                    if options.file.is_some_and(|file|file!=path) {continue;}
                    let mut located=BTreeSet::new();
                    add_ids(
                        conn,
                        "SELECT id FROM knowledge_symbols WHERE path=?1 AND line=?2 AND name=?3",
                        params![path, integer(*line).map_err(unreadable)?, name],
                        &mut located,
                        budget,
                        &mut omitted,
                    )?;
                    for id in located {if discovered.insert(id.clone()){discovery_order.push(id);}}
                    if discovery_order.len()>=budget {omitted=true;break;}
                }
            }
            let expression = fts_query(&terms, "OR");
            if options.text.trim().is_empty() {
                add_ids(
                    conn,
                    "SELECT id FROM knowledge_symbols WHERE (?1 IS NULL OR path=?1) ORDER BY path,line",
                    params![options.file],
                    &mut ids,
                    if options.all { usize::MAX } else { options.limit.clamp(1, 50) },
                    &mut omitted,
                )?;
            } else if !expression.is_empty() {
                let mut statement=conn.prepare("SELECT s.id FROM knowledge_fts JOIN knowledge_symbols s ON s.rowid=knowledge_fts.rowid WHERE knowledge_fts MATCH ?1 AND (?2 IS NULL OR s.path=?2) ORDER BY bm25(knowledge_fts,5,2,0.25,1,0),s.path,s.line").map_err(|e|unreadable(e.into()))?;
                let rows=statement.query_map(params![expression,options.file],|r|r.get::<_,String>(0)).map_err(|e|unreadable(e.into()))?;
                let mut lexical=Vec::new();
                for row in rows {if lexical.len()>=budget {omitted=true;break;}lexical.push(row.map_err(|e|unreadable(e.into()))?);}
                // Neither reservoir can consume the entire hydration budget
                // before the other contributes its source evidence.
                for at in 0..lexical.len().max(discovery_order.len()) {
                    for id in [lexical.get(at),discovery_order.get(at)].into_iter().flatten() {
                        if !ids.contains(id) && ids.len()>=budget {omitted=true;continue;}
                        ids.insert(id.clone());
                    }
                }
            } else {
                for id in discovery_order {
                    if !ids.contains(&id) && ids.len()>=budget {omitted=true;continue;}
                    ids.insert(id);
                }
            }
        }
    }
    let cards = hydrate(conn, &ids).map_err(unreadable)?;
    let outdated: i64 = conn
        .query_row("SELECT count(*) FROM knowledge_files WHERE evidence_version!=?1", [integer(knowledge::VERSION).map_err(unreadable)?], |r| r.get(0))
        .map_err(|e| unreadable(e.into()))?;
    Ok(Pool { cards, total: usize::try_from(total).unwrap_or(0), omitted, outdated })
}

fn add_ids(
    conn: &Connection,
    sql: &str,
    parameters: impl rusqlite::Params,
    ids: &mut BTreeSet<String>,
    max: usize,
    omitted: &mut bool,
) -> std::result::Result<(), MapRefusal> {
    let mut statement = conn.prepare(sql).map_err(|e| unreadable(e.into()))?;
    let rows = statement.query_map(parameters, |r| r.get::<_, String>(0)).map_err(|e| unreadable(e.into()))?;
    for row in rows {
        let id = row.map_err(|e| unreadable(e.into()))?;
        if !ids.contains(&id) && ids.len() >= max {
            *omitted = true;
            break;
        }
        ids.insert(id);
    }
    Ok(())
}

/// Revisit declarations inside already selected files. Each file gets its
/// own indexed reservoir, so broad repository matches cannot hide its method.
pub(super) fn expand_files(root:&Path,cards:&mut Vec<Card>,files:&[String],query:&str,all:bool)->std::result::Result<(usize,bool),MapRefusal> {
    if all || files.is_empty() {return Ok((0,false));}
    let db=store::open_existing(&store::model_path(root))?;
    let terms=query_terms(query,&Languages::of_project(root));
    let expression=format!("{{name own intent}} : {}",fts_query(&terms,"OR"));
    if terms.is_empty(){return Ok((0,false));}
    let per_file=512usize.div_ceil(files.len());
    let mut ids=BTreeSet::new();let mut omitted=false;
    for file in files {
        let mut scoped=BTreeSet::new();
        add_ids(db.conn(),"SELECT s.id FROM knowledge_fts JOIN knowledge_symbols s ON s.rowid=knowledge_fts.rowid WHERE knowledge_fts MATCH ?1 AND s.path=?2 ORDER BY bm25(knowledge_fts,5,2,0.25,1,0),s.line",
            params![expression,file],&mut scoped,per_file,&mut omitted)?;
        ids.extend(scoped);
    }
    for card in cards.iter(){ids.remove(&card.id);}
    let extra=hydrate(db.conn(),&ids).map_err(unreadable)?;
    let added=extra.len();cards.extend(extra);
    Ok((added,omitted))
}

pub(super) fn hydrate(conn: &Connection, ids: &BTreeSet<String>) -> Result<Vec<Card>> {
    // Parse a file's evidence pack once, rather than asking SQLite to parse
    // the entire JSON again for every matching declaration in that file.
    let mut statement=conn.prepare("SELECT path,position,id FROM knowledge_symbols WHERE id IN (SELECT value FROM json_each(?1)) ORDER BY path,position")?;
    let inventory=serde_json::to_string(ids).map_err(|e|Error::Parse(e.to_string()))?;
    let rows=statement.query_map([inventory],|row|Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?,row.get::<_,String>(2)?)))?;
    let mut files=std::collections::BTreeMap::<String,Vec<(usize,String)>>::new();
    for row in rows {let (path,position,id)=row?;let position=usize::try_from(position).map_err(|e|Error::Parse(e.to_string()))?;files.entry(path).or_default().push((position,id));}
    let mut cards = Vec::new();
    for (path,positions) in files {
        let text:Option<String>=conn.query_row("SELECT analysis FROM texts WHERE path=?1",[path],|row|row.get(0)).optional()?.flatten();
        if let Some(text) = text {
            let mut pack:Value=serde_json::from_str(&text).map_err(|e|Error::Parse(e.to_string()))?;
            for (position,id) in positions {
                let value=pack["knowledge"]["cards"].as_array_mut().and_then(|cards|cards.get_mut(position)).map(Value::take)
                    .ok_or_else(||Error::Parse("knowledge-catalog-position-mismatch; refresh scan".into()))?;
                let card:Card=serde_json::from_value(value).map_err(|e|Error::Parse(e.to_string()))?;
                if card.id != id {return Err(Error::Parse("knowledge-catalog-identity-mismatch; refresh scan".into()));}
                cards.push(card);
            }
        }
    }
    cards.sort_by(|a, b| (&a.source.file, a.source.line, &a.id).cmp(&(&b.source.file, b.source.line, &b.id)));
    Ok(cards)
}

pub(super) fn neighbors_in(
    root: &Path, cards: &mut Vec<Card>, seeds: &[usize], depth: usize,
    direction: super::Direction, all: bool, scope: Option<&super::EvidenceScope<'_>>,
) -> std::result::Result<bool, MapRefusal> {
    let db = store::open_existing(&store::model_path(root))?;
    let conn = db.conn();
    let mut known: BTreeSet<_> = cards.iter().map(|c| c.id.clone()).collect();
    let mut frontier: BTreeSet<_> = seeds.iter().map(|&i| cards[i].id.clone()).collect();
    let mut omitted = false;
    let budget = if all { usize::MAX } else { 512 };
    let inventory = scope.map(|scope| serde_json::to_string(scope.files)).transpose().map_err(|e| super::invalid(e.to_string()))?;
    for _ in 0..=depth.min(4) {
        let mut ids = BTreeSet::new();
        for id in frontier {
            if direction != super::Direction::Callers {
                add_ids(conn, "SELECT l.target FROM knowledge_links l JOIN knowledge_symbols s ON s.id=l.target WHERE l.source=?1 AND (?2 IS NULL OR s.path IN (SELECT value FROM json_each(?2))) ORDER BY l.target", params![id, inventory], &mut ids, budget, &mut omitted)?;
            }
            if direction != super::Direction::Outgoing {
                add_ids(conn, "SELECT l.source FROM knowledge_links l JOIN knowledge_symbols s ON s.id=l.source WHERE l.target=?1 AND (?2 IS NULL OR s.path IN (SELECT value FROM json_each(?2))) ORDER BY l.source", params![id, inventory], &mut ids, budget, &mut omitted)?;
            }
        }
        ids.retain(|id| !known.contains(id));
        if ids.is_empty() {
            break;
        }
        let extra = hydrate(conn, &ids).map_err(unreadable)?;
        frontier = extra.iter().map(|c| c.id.clone()).collect();
        known.extend(frontier.iter().cloned());
        cards.extend(extra);
    }
    Ok(omitted)
}

/// Alias alternatives stay in one slot; the caller chooses AND or OR
/// between informative slots. Quotes are data, never FTS query syntax.
pub(super) fn fts_query(terms: &[Vec<String>], join: &str) -> String {
    terms
        .iter()
        .filter(|slot| !slot.is_empty())
        .map(|slot| {
            format!(
                "({})",
                slot.iter().map(|term| format!("\"{}\"", term.replace('"', "\"\""))).collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>().join(" OR ")
            )
        })
        .collect::<Vec<_>>()
        .join(&format!(" {join} "))
}

pub(super) fn integer(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|e| Error::Parse(e.to_string()))
}
