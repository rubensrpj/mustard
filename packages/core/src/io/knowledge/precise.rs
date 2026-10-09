//! Optional SCIP import and indexed symbol resolution. Imports require source
//! text embedded by the producer or a manifest bound to the exact index bytes.
//! Every participating source is revalidated before global resolution.
use crate::domain::knowledge::precise::{
    Location, PreciseSymbols, Reference, Relation, Resolution,
};
use crate::domain::knowledge::{Source, resources::Registry};
use crate::io::{
    map_db::{Block, Kind, MapDb},
    project_map as store,
    sha256::Sha256,
};
use prost::Message;
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
pub mod lsp;
mod wire;

pub(crate) const BLOCK: Block = Block {
    name: "precise_symbols",
    version: 1,
    tables: &[
        "precise_documents",
        "precise_occurrences",
        "precise_relationships",
        "precise_meta",
    ],
    schema: "CREATE TABLE IF NOT EXISTS precise_documents(path TEXT PRIMARY KEY,sha256 TEXT NOT NULL);\
    CREATE TABLE IF NOT EXISTS precise_occurrences(path TEXT NOT NULL,line INTEGER NOT NULL,col INTEGER NOT NULL,end_line INTEGER NOT NULL,end_col INTEGER NOT NULL,symbol TEXT NOT NULL,roles INTEGER NOT NULL);\
    CREATE INDEX IF NOT EXISTS precise_position ON precise_occurrences(path,line,col);\
    CREATE INDEX IF NOT EXISTS precise_symbol ON precise_occurrences(symbol,path,line,col);\
    CREATE TABLE IF NOT EXISTS precise_relationships(owner TEXT NOT NULL,target TEXT NOT NULL,is_ref INTEGER NOT NULL,is_impl INTEGER NOT NULL,is_def INTEGER NOT NULL);\
    CREATE INDEX IF NOT EXISTS precise_related ON precise_relationships(owner,target);\
    CREATE INDEX IF NOT EXISTS precise_inverse ON precise_relationships(target,owner);\
    CREATE TABLE IF NOT EXISTS precise_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);",
    kind: Kind::Written(convert),
};
fn convert(conn: &rusqlite::Connection, _version: u32) -> crate::platform::error::Result<()> {
    conn.execute_batch(BLOCK.schema)?;
    Ok(())
}
fn hash(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.hex_digest()
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub index_sha256: String,
    pub sources: BTreeMap<String, String>,
    /// Required only for old producers that omit Document.position_encoding.
    pub position_encoding: Option<i32>,
}
fn key(file: &str, symbol: &str) -> String {
    if symbol.starts_with("local ") {
        format!("{file}\0{symbol}")
    } else {
        symbol.into()
    }
}
pub(super) fn byte_column(line: &str, column: i32, encoding: i32) -> Result<u64, String> {
    let col = usize::try_from(column).map_err(|_| "precise-negative-position")?;
    if encoding == 1 {
        if col <= line.len() && line.is_char_boundary(col) {
            return Ok(col as u64);
        }
    } else if matches!(encoding, 2 | 3) {
        let mut units = 0;
        for (at, c) in line.char_indices() {
            if units == col {
                return Ok(at as u64);
            }
            units += if encoding == 2 { c.len_utf16() } else { 1 };
        }
        if units == col {
            return Ok(line.len() as u64);
        }
    }
    Err("precise-invalid-position-or-encoding".into())
}
fn range(
    text: &str,
    item: &wire::Occurrence,
    encoding: i32,
) -> Result<(u64, u64, u64, u64), String> {
    let coords = match &item.typed_range {
        Some(wire::TypedRange::Single(r)) => [r.line, r.start, r.line, r.end],
        Some(wire::TypedRange::Multi(r)) => [r.start_line, r.start, r.end_line, r.end],
        None => match item.range.as_slice() {
            [l, c, e] => [*l, *c, *l, *e],
            [l, c, el, ec] => [*l, *c, *el, *ec],
            _ => return Err("precise-invalid-range".into()),
        },
    };
    let [l, c, el, ec] = coords;
    if l < 0 || el < l {
        return Err("precise-invalid-range".into());
    }
    // split includes a final empty line, valid for an exclusive EOF position.
    let lines: Vec<_> = text
        .split('\n')
        .map(|s| s.strip_suffix('\r').unwrap_or(s))
        .collect();
    let col = byte_column(
        lines.get(l as usize).ok_or("precise-line-outside-source")?,
        c,
        encoding,
    )?;
    let end_col = byte_column(
        lines
            .get(el as usize)
            .ok_or("precise-line-outside-source")?,
        ec,
        encoding,
    )?;
    if l == el && col > end_col {
        return Err("precise-reversed-range".into());
    }
    Ok((l as u64 + 1, col, el as u64 + 1, end_col))
}

pub fn import(
    root: &Path,
    tree: &Path,
    index_path: &Path,
    manifest: Option<&Path>,
) -> Result<Value, String> {
    if std::fs::metadata(index_path)
        .map_err(|e| e.to_string())?
        .len()
        > 64 * 1024 * 1024
    {
        return Err("precise-index-too-large".into());
    }
    let bytes = std::fs::read(index_path).map_err(|e| e.to_string())?;
    let digest = hash(&bytes);
    let manifest: Option<Manifest> = manifest
        .map(|path| {
            if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 4 * 1024 * 1024 {
                return Err("precise-manifest-too-large".into());
            }
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())
        })
        .transpose()?;
    if manifest.as_ref().is_some_and(|m| m.index_sha256 != digest) {
        return Err("precise-manifest-index-mismatch".into());
    }
    let index = wire::Index::decode(bytes.as_slice()).map_err(|e| e.to_string())?;
    let metadata = index.metadata.ok_or("precise-metadata-required")?;
    if metadata.text_document_encoding != 1 {
        return Err("precise-source-encoding-must-be-utf8".into());
    }
    let tool = metadata
        .tool_info
        .filter(|t| !t.name.trim().is_empty())
        .ok_or("precise-producer-required")?;
    let producer = format!("{} {}", tool.name, tool.version).trim().to_string();
    let registry = Registry::load()?;
    let mut sources = BTreeMap::new();
    let mut occurrences = Vec::new();
    let mut relationships = Vec::new();
    for doc in index.documents {
        let file = &doc.relative_path;
        if file.contains('\\')
            || file.split('/').any(|part| matches!(part, "" | "." | ".."))
            || !std::fs::symlink_metadata(tree.join(file)).is_ok_and(|m| m.is_file())
        {
            return Err("precise-document-path-must-be-canonical-regular-file".into());
        }
        if sources.contains_key(file) {
            return Err("precise-duplicate-document".into());
        }
        let text = super::investigation::safe_read(tree, file, &registry)
            .ok_or("precise-source-excluded-or-unreadable")?;
        let sha256 = hash(text.as_bytes());
        let embedded = !doc.text.is_empty() && doc.text == text;
        let receipted = manifest.as_ref().and_then(|m| m.sources.get(file)) == Some(&sha256);
        if !embedded && !receipted {
            return Err(format!(
                "precise-source-receipt-required-or-changed: {file}"
            ));
        }
        let encoding = if doc.position_encoding == 0 {
            manifest
                .as_ref()
                .and_then(|m| m.position_encoding)
                .ok_or("precise-position-encoding-required")?
        } else {
            doc.position_encoding
        };
        for occurrence in doc.occurrences {
            if occurrence.symbol.is_empty() {
                continue;
            }
            let (line, col, end_line, end_col) = range(&text, &occurrence, encoding)?;
            occurrences.push((
                file.clone(),
                line,
                col,
                end_line,
                end_col,
                key(file, &occurrence.symbol),
                occurrence.roles,
            ));
        }
        for symbol in doc.symbols {
            for related in symbol.relationships {
                relationships.push((
                    key(file, &symbol.symbol),
                    key(file, &related.symbol),
                    related.is_reference,
                    related.is_implementation,
                    related.is_definition,
                ));
            }
        }
        sources.insert(file.clone(), sha256);
    }
    if sources.is_empty() {
        return Err("precise-no-documents".into());
    }
    let indexed_documents = sources.len();
    // Producers can bind build configuration and lockfiles in the same source
    // manifest. They carry no occurrences, but changing one invalidates the
    // entire index, just like changing an indexed declaration.
    if let Some(manifest) = &manifest {
        for (file, expected) in &manifest.sources {
            if sources.contains_key(file) {
                continue;
            }
            let text = super::investigation::safe_read(tree, file, &registry)
                .ok_or("precise-context-source-unavailable")?;
            let digest = hash(text.as_bytes());
            if digest != *expected {
                return Err("precise-context-source-changed".into());
            }
            sources.insert(file.clone(), digest);
        }
    }

    // Recheck after decoding/conversion and immediately before the transaction.
    for (file, sha256) in &sources {
        if super::investigation::safe_read(tree, file, &registry)
            .is_none_or(|s| hash(s.as_bytes()) != *sha256)
        {
            return Err("precise-source-changed-during-import".into());
        }
    }
    let mut db = MapDb::open_waiting(
        &store::model_path(root),
        root,
        &store::DB_BLOCKS,
        std::time::Duration::from_millis(100),
    )
    .map_err(|e| e.to_string())?;
    db.write(|tx| {
        tx.execute_batch("DELETE FROM precise_documents; DELETE FROM precise_occurrences; DELETE FROM precise_relationships; DELETE FROM precise_meta;")?;
        for (file,sha256) in &sources {tx.execute("INSERT INTO precise_documents VALUES(?1,?2)",params![file,sha256])?;}
        for (file,line,col,el,ec,symbol,roles) in &occurrences {tx.execute("INSERT INTO precise_occurrences VALUES(?1,?2,?3,?4,?5,?6,?7)",params![file,*line as i64,*col as i64,*el as i64,*ec as i64,symbol,roles])?;}
        for (owner,target,is_ref,is_impl,is_def) in &relationships {tx.execute("INSERT INTO precise_relationships VALUES(?1,?2,?3,?4,?5)",params![owner,target,is_ref,is_impl,is_def])?;}
        for (k,v) in [("producer",producer.as_str()),("index_sha256",digest.as_str())] {tx.execute("INSERT INTO precise_meta VALUES(?1,?2)",params![k,v])?;}
        crate::io::map_revision::bump(tx)?;
        Ok(())
    }).map_err(|e|e.to_string())?;
    Ok(
        json!({"ok":true,"producer":producer,"index_sha256":digest,"documents":indexed_documents,"context_files":sources.len()-indexed_documents,"occurrences":occurrences.len(),
        "source_freshness":"all imported documents verified","precision":"producer-dependent; not runtime proof","local_model_calls":0,"remote_model_calls":0}),
    )
}

pub struct ScipSymbols<'a> {
    pub root: &'a Path,
    pub tree: &'a Path,
}
impl PreciseSymbols for ScipSymbols<'_> {
    fn resolve(
        &self,
        location: &Location<'_>,
        relation: Relation,
        limit: usize,
    ) -> Result<Resolution, String> {
        if location.line == 0 {
            return Err("precise-line-starts-at-one".into());
        }
        let db =
            super::open_existing(&store::model_path(self.root)).map_err(|e| format!("{e:?}"))?;
        let conn = db
            .conn()
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        let producer: Option<String> = conn
            .query_row(
                "SELECT value FROM precise_meta WHERE key='producer'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(producer) = producer else {
            return Ok(Resolution {
                status: "index-unavailable; use native search".into(),
                producer: String::new(),
                index_sha256: String::new(),
                references: vec![],
                symbols: vec![],
                has_more: false,
            });
        };
        let digest = conn
            .query_row(
                "SELECT value FROM precise_meta WHERE key='index_sha256'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let mut result = Resolution {
            status: "current-index".into(),
            producer,
            index_sha256: digest,
            references: vec![],
            symbols: vec![],
            has_more: false,
        };
        let registry = Registry::load()?;
        let mut stmt = conn
            .prepare("SELECT path,sha256 FROM precise_documents")
            .map_err(|e| e.to_string())?;
        let sources = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<BTreeMap<_, _>>>()
            .map_err(|e| e.to_string())?;
        for (file, sha256) in &sources {
            if super::investigation::safe_read(self.tree, file, &registry)
                .is_none_or(|s| hash(s.as_bytes()) != *sha256)
            {
                result.status =
                    "index-stale; use native search and rebuild the precise index".into();
                return Ok(result);
            }
        }
        let mut stmt=conn.prepare("SELECT DISTINCT symbol FROM precise_occurrences WHERE path=?1 AND (line<?2 OR line=?2 AND col<=?3) AND (end_line>?2 OR end_line=?2 AND end_col>?3) ORDER BY symbol").map_err(|e|e.to_string())?;
        let seeds = stmt
            .query_map(
                params![
                    location.file,
                    i64::try_from(location.line).map_err(|_| "precise-invalid-line")?,
                    i64::try_from(location.column_bytes).map_err(|_| "precise-invalid-column")?
                ],
                |r| r.get::<_, String>(0),
            )
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<BTreeSet<_>>>()
            .map_err(|e| e.to_string())?;
        result.symbols = seeds.iter().cloned().collect();
        let mut symbols = seeds.clone();
        let mut stmt = conn
            .prepare("SELECT owner,target,is_ref,is_impl,is_def FROM precise_relationships")
            .map_err(|e| e.to_string())?;
        let links = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, bool>(2)?,
                    r.get::<_, bool>(3)?,
                    r.get::<_, bool>(4)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?;
        for pass in 0..32 {
            let before = symbols.len();
            for (owner, target, is_ref, is_impl, is_def) in &links {
                match relation {
                    Relation::References if *is_ref => {
                        if symbols.contains(owner) {
                            symbols.insert(target.clone());
                        }
                        if symbols.contains(target) {
                            symbols.insert(owner.clone());
                        }
                    }
                    Relation::Definitions if *is_def && symbols.contains(owner) => {
                        symbols.insert(target.clone());
                    }
                    Relation::Implementations if *is_impl && symbols.contains(target) => {
                        symbols.insert(owner.clone());
                    }
                    _ => {}
                }
            }
            if symbols.len() == before {
                break;
            }
            if pass == 31 {
                result.has_more = true;
            }
        }
        if relation == Relation::Implementations {
            symbols = symbols.difference(&seeds).cloned().collect();
        }
        let limit = limit.clamp(1, 256);
        for symbol in symbols {
            let mut stmt=conn.prepare("SELECT path,line,col,end_line,end_col,roles FROM precise_occurrences WHERE symbol=?1 ORDER BY path,line,col").map_err(|e|e.to_string())?;
            let rows = stmt
                .query_map([&symbol], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, u32>(1)?,
                        r.get::<_, u32>(2)?,
                        r.get::<_, u32>(3)?,
                        r.get::<_, u32>(4)?,
                        r.get::<_, i32>(5)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            for row in rows {
                let (file, line, col, end_line, end_col, roles) = row.map_err(|e| e.to_string())?;
                let (line, col, end_line, end_col) = (
                    u64::from(line),
                    u64::from(col),
                    u64::from(end_line),
                    u64::from(end_col),
                );
                let definition = roles & (1 | 64) != 0;
                if relation == Relation::References && definition
                    || relation != Relation::References && !definition
                {
                    continue;
                }
                if result.references.len() >= limit {
                    result.has_more = true;
                    continue;
                }
                let receipt_end = if end_col == 0 && end_line > line {
                    end_line - 1
                } else {
                    end_line
                };
                result.references.push(Reference {
                    symbol: symbol.clone(),
                    source: Source {
                        file: file.clone(),
                        line,
                        end_line: receipt_end,
                        sha256: sources[&file].clone(),
                    },
                    column_bytes: col,
                    end_line,
                    end_column_bytes: end_col,
                    roles,
                });
            }
        }
        if result.symbols.is_empty() {
            result.status = "no-symbol-at-position; use native search".into();
        }
        for (file, sha256) in &sources {
            if super::investigation::safe_read(self.tree, file, &registry)
                .is_none_or(|s| hash(s.as_bytes()) != *sha256)
            {
                result.status = "index-changed-during-query; use native search".into();
                result.references.clear();
                result.symbols.clear();
                break;
            }
        }
        Ok(result)
    }
}
pub fn references(
    root: &Path,
    tree: &Path,
    file: &str,
    line: u64,
    column: u64,
    relation: &str,
    limit: usize,
) -> Result<Value, String> {
    let location = Location {
        file,
        line,
        column_bytes: column,
    };
    let relation = Relation::parse(relation)?;
    let indexed = ScipSymbols { root, tree }.resolve(&location, relation, limit);
    // A fresh imported index is the cheapest precise provider. Never reuse a
    // stale result or ask a classifier to guess a symbol's identity.
    if let Ok(result) = &indexed
        && result.status == "current-index"
    {
        return serde_json::to_value(result).map_err(|e| e.to_string());
    }
    let previous = indexed.map_or_else(|e| e, |r| r.status);
    match (lsp::LspSymbols { tree }).resolve(&location, relation, limit) {
        Ok(result) => {
            let mut result = serde_json::to_value(result).map_err(|e| e.to_string())?;
            result["index_status"] = json!(previous);
            Ok(result)
        }
        Err(reason) => Ok(
            json!({"status":"precise-unavailable; use native search","index_status":previous,"reason":reason,"references":[],"symbols":[],"has_more":false}),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn occurrence(line: i32, start: i32, end: i32, symbol: &str, roles: i32) -> wire::Occurrence {
        wire::Occurrence {
            range: vec![],
            symbol: symbol.into(),
            roles,
            typed_range: Some(wire::TypedRange::Single(wire::SingleLine {
                line,
                start,
                end,
            })),
        }
    }
    fn fixture() -> (tempfile::TempDir, wire::Index) {
        let dir = tempfile::tempdir().unwrap();
        let a = "😀foo\nfoo\nfoo\n";
        let b = "bar\nbar\n";
        std::fs::write(dir.path().join("a.ts"), a).unwrap();
        std::fs::write(dir.path().join("b.rs"), b).unwrap();
        let index = wire::Index {
            metadata: Some(wire::Metadata {
                tool_info: Some(wire::ToolInfo {
                    name: "fixture-indexer".into(),
                    version: "1".into(),
                }),
                project_root: String::new(),
                text_document_encoding: 1,
            }),
            documents: vec![
                wire::Document {
                    relative_path: "a.ts".into(),
                    text: a.into(),
                    position_encoding: 2,
                    symbols: vec![],
                    occurrences: vec![
                        occurrence(0, 2, 5, "local 0", 1),
                        occurrence(1, 0, 3, "local 0", 0),
                        occurrence(2, 0, 3, "local 0", 0),
                    ],
                },
                wire::Document {
                    relative_path: "b.rs".into(),
                    text: b.into(),
                    position_encoding: 1,
                    symbols: vec![],
                    occurrences: vec![
                        occurrence(0, 0, 3, "local 0", 1),
                        occurrence(1, 0, 3, "local 0", 0),
                    ],
                },
            ],
        };
        (dir, index)
    }
    fn save(root: &Path, index: &wire::Index) -> std::path::PathBuf {
        let path = root.join("index.scip");
        std::fs::write(&path, index.encode_to_vec()).unwrap();
        path
    }
    #[test]
    fn import_preserves_typed_unicode_ranges_local_identity_and_pagination() {
        let (dir, mut index) = fixture();
        let root = dir.path();
        // A modern typed range takes precedence over the historical field.
        index.documents[0].occurrences[0].range = vec![999, 999, 999];
        import(root, root, &save(root, &index), None).unwrap();
        let port = ScipSymbols { root, tree: root };
        let location = Location {
            file: "a.ts",
            line: 2,
            column_bytes: 1,
        };
        let defs = port.resolve(&location, Relation::Definitions, 64).unwrap();
        assert_eq!(defs.references.len(), 1);
        assert_eq!(defs.references[0].column_bytes, 4);
        assert_eq!(defs.references[0].end_column_bytes, 7);
        let refs = port.resolve(&location, Relation::References, 1).unwrap();
        assert_eq!(refs.references.len(), 1);
        assert!(refs.has_more);
        assert_eq!(refs.references[0].source.file, "a.ts");
        assert!(
            port.resolve(
                &Location {
                    file: "a.ts",
                    line: 1,
                    column_bytes: 5
                },
                Relation::References,
                64
            )
            .unwrap()
            .references
            .iter()
            .all(|r| r.source.file == "a.ts")
        );
    }
    #[test]
    fn failed_import_is_atomic_and_other_source_edits_invalidate_the_index() {
        let (dir, mut index) = fixture();
        let root = dir.path();
        import(root, root, &save(root, &index), None).unwrap();
        index.documents[1].text = "an outdated source".into();
        assert!(
            import(root, root, &save(root, &index), None)
                .unwrap_err()
                .contains("receipt")
        );
        let port = ScipSymbols { root, tree: root };
        let location = Location {
            file: "a.ts",
            line: 2,
            column_bytes: 1,
        };
        assert_eq!(
            port.resolve(&location, Relation::Definitions, 64)
                .unwrap()
                .references
                .len(),
            1
        );
        std::fs::write(root.join("b.rs"), "bar changed\n").unwrap();
        let stale = port.resolve(&location, Relation::Definitions, 64).unwrap();
        assert!(stale.status.starts_with("index-stale"));
        assert!(stale.references.is_empty());
    }
    #[test]
    fn detached_indexes_require_a_manifest_bound_to_index_and_sources() {
        let (dir, mut index) = fixture();
        let root = dir.path();
        for doc in &mut index.documents {
            doc.text.clear();
            doc.position_encoding = 0;
        }
        let path = save(root, &index);
        assert!(import(root, root, &path, None).is_err());
        let manifest_path = root.join("manifest.json");
        std::fs::write(root.join("package.json"), "{\"type\":\"module\"}\n").unwrap();
        let mut manifest = json!({"index_sha256":hash(&index.encode_to_vec()),"position_encoding":2,
            "sources":{"a.ts":hash(&std::fs::read(root.join("a.ts")).unwrap()),"b.rs":hash(&std::fs::read(root.join("b.rs")).unwrap()),
                "package.json":hash(&std::fs::read(root.join("package.json")).unwrap())}});
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        let imported = import(root, root, &path, Some(&manifest_path)).unwrap();
        assert_eq!(imported["documents"], 2);
        assert_eq!(imported["context_files"], 1);
        let location = Location {
            file: "a.ts",
            line: 2,
            column_bytes: 1,
        };
        let port = ScipSymbols { root, tree: root };
        assert_eq!(
            port.resolve(&location, Relation::Definitions, 64)
                .unwrap()
                .status,
            "current-index"
        );
        std::fs::write(root.join("package.json"), "{\"type\":\"commonjs\"}\n").unwrap();
        let stale = port.resolve(&location, Relation::Definitions, 64).unwrap();
        assert!(stale.status.starts_with("index-stale"));
        assert!(stale.references.is_empty());
        manifest["index_sha256"] = json!("another-index");
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        assert!(
            import(root, root, &path, Some(&manifest_path))
                .unwrap_err()
                .contains("index-mismatch")
        );
        index.documents[0].relative_path = "../a.ts".into();
        assert!(import(root, root, &save(root, &index), None).is_err());
    }
    #[test]
    fn relationships_resolve_implementations_without_conflating_local_symbols() {
        let (dir, mut index) = fixture();
        let root = dir.path();
        index.documents[0].symbols.push(wire::Symbol {
            symbol: "local 0".into(),
            relationships: vec![wire::Relationship {
                symbol: "scip example Base#method().".into(),
                is_reference: true,
                is_implementation: true,
                is_definition: false,
            }],
        });
        index.documents[1].occurrences[0].symbol = "scip example Base#method().".into();
        index.documents[1].occurrences[1].symbol = "scip example Base#method().".into();
        import(root, root, &save(root, &index), None).unwrap();
        let port = ScipSymbols { root, tree: root };
        let location = Location {
            file: "b.rs",
            line: 1,
            column_bytes: 1,
        };
        let implemented = port
            .resolve(&location, Relation::Implementations, 64)
            .unwrap();
        assert_eq!(implemented.references.len(), 1);
        assert_eq!(implemented.references[0].source.file, "a.ts");
        let references = port.resolve(&location, Relation::References, 64).unwrap();
        assert_eq!(references.references.len(), 3);
    }
}
