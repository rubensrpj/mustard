//! Native knowledge retrieval and versioned, multi-source interpretations.
//! Source freshness is necessary, but never a semantic proof of the claim.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::domain::knowledge::{self, Card, Source};
use crate::domain::normalize::Languages;
use crate::domain::project_map::{MapRefusal, ProjectMap};
use crate::io::project_map::{self as store, open_existing, unreadable};
use crate::io::sha256::Sha256;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interpretation {
    pub id: String,
    pub title: String,
    pub text: String,
    /// `hypothesis` or `reviewed`; a receipt never upgrades either status.
    pub status: String,
    /// Origin supplied by the person/agent that actually read the evidence.
    pub origin: String,
    pub sources: Vec<Source>,
}

fn invalid(detail: impl Into<String>) -> MapRefusal {
    MapRefusal::MapUnreadable {
        detail: detail.into(),
    }
}

fn source_bytes(root: &Path, file: &str) -> Option<Vec<u8>> {
    let path = Path::new(file);
    if path.is_absolute()
        || file.contains('\\')
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let path = root.join(path).canonicalize().ok()?;
    if !path.starts_with(root) {
        return None;
    }
    std::fs::read(path).ok()
}

fn current(
    root: &Path,
    source: &Source,
    hashes: &mut BTreeMap<String, Option<(String, u64)>>,
) -> bool {
    let now = hashes.entry(source.file.clone()).or_insert_with(|| {
        let bytes = source_bytes(root, &source.file)?;
        let mut hash = Sha256::new();
        hash.update(&bytes);
        let lines = String::from_utf8_lossy(&bytes).lines().count() as u64;
        Some((hash.hex_digest(), lines))
    });
    now.as_ref().is_some_and(|(hash, lines)| {
        source.sha256.len() == 64
            && hash == &source.sha256
            && source.line > 0
            && source.end_line >= source.line
            && source.end_line <= *lines
    })
}

pub fn interpretations(root: &Path) -> Result<Vec<Interpretation>, MapRefusal> {
    let db = open_existing(&store::model_path(root))?;
    let mut statement = db
        .conn()
        .prepare("SELECT payload FROM knowledge_notes ORDER BY id")
        .map_err(|e| unreadable(e.into()))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| unreadable(e.into()))?;
    rows.map(|row| {
        let text = row.map_err(|e| unreadable(e.into()))?;
        serde_json::from_str(&text).map_err(|e| invalid(e.to_string()))
    })
    .collect()
}

/// All supplied sources must still match. No silent re-stamping of an old
/// interpretation after a source changes. A stable id replaces its own note.
pub fn record(root: &Path, note: &Interpretation) -> Result<Value, MapRefusal> {
    record_at(root, root, note)
}

/// Store at the project anchor, but validate the actual checkout that the
/// reviewer inspected. A wave's changed evidence is not current in main.
pub fn record_at(root: &Path, tree: &Path, note: &Interpretation) -> Result<Value, MapRefusal> {
    if note.id.is_empty()
        || note.id.len() > 120
        || !note
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        || note.title.trim().is_empty()
        || note.title.len() > 400
        || note.text.trim().is_empty()
        || note.text.len() > 12000
        || note.origin.trim().is_empty()
        || note.origin.len() > 400
        || !["hypothesis", "reviewed"].contains(&note.status.as_str())
        || note.sources.is_empty()
        || note.sources.len() > 32
    {
        return Err(invalid("knowledge-invalid-interpretation"));
    }
    let mut hashes = BTreeMap::new();
    if note
        .sources
        .iter()
        .any(|source| !current(tree, source, &mut hashes))
    {
        return Err(invalid("knowledge-source-changed-or-invalid"));
    }
    let payload = serde_json::to_string(note).map_err(|e| invalid(e.to_string()))?;
    let mut db = open_existing(&store::model_path(root))?;
    db.write(|tx| {
        tx.execute("DELETE FROM knowledge_notes WHERE id=?1", params![note.id])?;
        tx.execute(
            "INSERT INTO knowledge_notes(id,payload) VALUES (?1,?2)",
            params![note.id, payload],
        )?;
        Ok(())
    })
    .map_err(unreadable)?;
    Ok(
        json!({"ok":true,"recorded":note.id,"status":note.status,"semantic_validation":"origin-review-only"}),
    )
}

/// Rank locally, follow only useful static links, then verify the content of
/// the selected sources. Reading/hashing in the binary sends zero source
/// tokens to a model. Outdated source text never becomes evidence for a spec.
pub fn query(
    root: &Path,
    query: &str,
    file: Option<&str>,
    limit: usize,
    depth: usize,
    all: bool,
) -> Result<(Value, ProjectMap), MapRefusal> {
    query_at(root, root, query, file, limit, depth, all)
}

pub fn query_at(
    root: &Path,
    tree: &Path,
    query: &str,
    file: Option<&str>,
    limit: usize,
    depth: usize,
    all: bool,
) -> Result<(Value, ProjectMap), MapRefusal> {
    let map = store::read(root)?;
    let mut cards = knowledge::cards(&map);
    let mut hashes = BTreeMap::new();
    let notes = interpretations(root)?;
    let (fresh, stale): (Vec<_>, Vec<_>) = notes.into_iter().partition(|note| {
        note.sources
            .iter()
            .all(|source| current(tree, source, &mut hashes))
    });
    let languages = Languages::of_project(root);
    let mut selected = BTreeSet::new();
    let mut order = Vec::new();
    let mut reasons = BTreeMap::new();
    let max = if all { cards.len() } else { limit.clamp(1, 50) };
    let note_documents: Vec<Card> = fresh
        .iter()
        .map(|note| Card {
            id: note.id.clone(),
            name: note.title.clone(),
            documentation: note.text.clone(),
            signature: String::new(),
            body_comment: String::new(),
            kind: "interpretation".into(),
            source: note.sources[0].clone(),
            literals: vec![],
            file_documentation: String::new(),
            parse_complete: None,
            contracts: vec![],
            routes: vec![],
            tests: vec![],
            inline_tests: false,
            outgoing: vec![],
            callers: vec![],
            unresolved_calls: 0,
        })
        .collect();
    let matching: Vec<_> = knowledge::ranked(&note_documents, query, &languages)
        .into_iter()
        .filter(|i| {
            file.is_none_or(|file| fresh[*i].sources.iter().any(|source| source.file == file))
        })
        .take(if all { usize::MAX } else { max })
        .collect();
    // Business interpretations lead back to all of their source functions.
    for &i in &matching {
        for source in &fresh[i].sources {
            for (n, card) in cards.iter().enumerate().filter(|(_, card)| {
                card.source.file == source.file
                    && card.source.line <= source.end_line
                    && source.line <= card.source.end_line
            }) {
                if selected.len() < max
                    && current(tree, &card.source, &mut hashes)
                    && selected.insert(n)
                {
                    order.push(n);
                    reasons.insert(n, "interpretation-source");
                }
            }
        }
    }
    let mut stale_cards = 0;
    let seed_limit = if all || depth == 0 {
        max
    } else {
        (max / 2).max(1)
    };
    let mut ranking = Vec::new();
    let mut local_hybrid_index = false;
    if selected.len() < seed_limit
        && !query.trim().is_empty()
        && let Ok(candidates) =
            crate::io::map_search::candidates(root, query, query, &languages, |path| {
                file.is_none_or(|file| file == path)
            })
    {
        let by_place: BTreeMap<_, _> = cards
            .iter()
            .enumerate()
            .map(|(i, card)| {
                (
                    (
                        card.source.file.as_str(),
                        card.source.line,
                        card.name.as_str(),
                    ),
                    i,
                )
            })
            .collect();
        let mut indexed = Vec::new();
        for candidate in candidates.candidates {
            if let Some(&i) = by_place.get(&(
                candidate.path.as_str(),
                u64::from(candidate.line),
                candidate.name.as_str(),
            )) && !indexed.contains(&i)
            {
                indexed.push(i);
            }
        }
        if !indexed.is_empty() {
            local_hybrid_index = true;
            ranking = indexed;
        }
    }
    if !local_hybrid_index && selected.len() < seed_limit {
        ranking = knowledge::ranked(&cards, query, &languages);
    }
    for i in ranking
        .into_iter()
        .filter(|i| file.is_none_or(|file| cards[*i].source.file == file))
    {
        if selected.len() >= seed_limit {
            break;
        }
        if current(tree, &cards[i].source, &mut hashes) {
            if selected.insert(i) {
                order.push(i);
                reasons.insert(
                    i,
                    if local_hybrid_index {
                        "local-hybrid-index"
                    } else {
                        "lexical"
                    },
                );
            }
        } else {
            stale_cards += 1;
        }
    }
    let by_id: BTreeMap<String, usize> = cards
        .iter()
        .enumerate()
        .map(|(i, card)| (card.id.clone(), i))
        .collect();
    let mut omitted_edges = 0;
    for _ in 0..depth.min(4) {
        let mut next = BTreeSet::new();
        for &i in &selected {
            for edge in &cards[i].outgoing {
                if let Some(&n) = edge["target"].as_str().and_then(|id| by_id.get(id)) {
                    if selected.contains(&n) || next.contains(&n) {
                        continue;
                    }
                    if selected.len() + next.len() < max
                        && current(tree, &cards[n].source, &mut hashes)
                    {
                        next.insert(n);
                    } else {
                        omitted_edges += 1;
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        for i in &next {
            order.push(*i);
            reasons.insert(*i, "static-relation");
        }
        selected.extend(next);
    }
    let mut result = Vec::new();
    let mut compacted_relations = 0;
    let mut stale_relations = 0;
    for i in order {
        let card = &mut cards[i];
        card.callers.retain(|edge| {
            let valid = edge
                .get("source")
                .and_then(|value| serde_json::from_value::<Source>(value.clone()).ok())
                .is_some_and(|source| current(tree, &source, &mut hashes));
            stale_relations += usize::from(!valid);
            valid
        });
        card.outgoing.retain(|edge| {
            let valid = edge
                .get("source")
                .and_then(|value| serde_json::from_value::<Source>(value.clone()).ok())
                .is_some_and(|source| current(tree, &source, &mut hashes));
            stale_relations += usize::from(!valid);
            valid
        });
        let outgoing = card.outgoing.len();
        let callers = card.callers.len();
        if !all {
            card.outgoing.truncate(8);
            card.callers.truncate(6);
            compacted_relations += outgoing - card.outgoing.len() + callers - card.callers.len();
        }
        let mut item = serde_json::to_value(&*card).map_err(|e| invalid(e.to_string()))?;
        if !all {
            for caller in item["callers"].as_array_mut().into_iter().flatten() {
                let count = caller["candidates"].as_array().map_or(0, Vec::len);
                caller["candidate_count"] = json!(count);
                if let Some(candidates) = caller["candidates"].as_array_mut() {
                    candidates.truncate(6);
                }
            }
        }
        item["retrieval"] = json!(reasons[&i]);
        item["current_outgoing_count"] = json!(outgoing);
        item["current_caller_count"] = json!(callers);
        result.push(item);
    }
    let mut gaps=vec!["Static relations do not prove runtime order, authorization, business rules or test coverage.".to_string()];
    if !stale.is_empty() {
        gaps.push(format!(
            "{} interpretations excluded: at least one source changed or disappeared.",
            stale.len()
        ));
    }
    if stale_cards > 0 {
        gaps.push(format!(
            "{stale_cards} matching declarations excluded: run scan to refresh changed sources."
        ));
    }
    if stale_relations > 0 {
        gaps.push(format!("{stale_relations} static relations excluded: caller or target source changed, disappeared or is invalid; absence is not proof of no consumers."));
    }
    if omitted_edges > 0 {
        gaps.push(format!("{omitted_edges} related declarations omitted by scope/depth/output or source validity; narrow the query or export --all."));
    }
    if compacted_relations > 0 {
        gaps.push(format!("{compacted_relations} current relations compacted from the response; use --file and --all for the full graph."));
    }
    if result.iter().any(|card| card["parse_complete"] != true) {
        gaps.push("Some selected sources have partial or unknown parse coverage.".into());
    }
    if result
        .iter()
        .any(|card| card["unresolved_calls"].as_u64().unwrap_or(0) > 0)
    {
        gaps.push("Some common-name calls were not resolved by the scanner.".into());
    }
    if result.is_empty() {
        gaps.push("No current evidence found. Use exact search or scan; absence is not proof that the capability does not exist.".into());
    }
    let interpretations: Vec<_> = matching.into_iter().map(|i| fresh[i].clone()).collect();
    let flows:Vec<_>=result.iter().filter(|card|card["routes"].as_array().is_some_and(|rows|!rows.is_empty()) || card["outgoing"].as_array().is_some_and(|rows|!rows.is_empty())).map(|card|
        json!({"entry":card["id"],"static_targets":card["outgoing"].as_array().into_iter().flatten().map(|edge|&edge["target"]).collect::<Vec<_>>(),"runtime_order":"unknown"})).collect();
    Ok((
        json!({"ok":true,"schema_version":knowledge::VERSION,"query":query,"cards":result,"interpretations":interpretations,
        "flows":flows,"scan_snapshot":{"head":map.state.head,"projects":map.projects,"languages":map.languages,
            "skeleton":map.skeleton.iter().filter(|layer|all || result.iter().any(|card|card["source"]["file"].as_str().is_some_and(|file|file.starts_with(&format!("{}/",layer.dir))))).collect::<Vec<_>>(),
            "freshness":"catalog-at-last-scan; selected sources individually verified"},
        "local_hybrid_index":local_hybrid_index,
        "stale_interpretations":stale.iter().map(|note|&note.id).collect::<Vec<_>>(),"gaps":gaps,
        "origin":"scan-and-versioned-interpretations","remote_model_calls":0}),
        map,
    ))
}

/// Narrow query for a prepared component: no full project deserialization or
/// graph ranking. All interpretation sources are checked in the wave copy.
pub fn for_source(root: &Path, tree: &Path, file: &str, name: &str) -> Value {
    let read = (|| -> Result<Value, MapRefusal> {
        let db = open_existing(&store::model_path(root))?;
        let analysis: Option<String> = db
            .conn()
            .query_row("SELECT analysis FROM texts WHERE path=?1", [file], |row| {
                row.get(0)
            })
            .map_err(|e| unreadable(e.into()))?;
        let Some(analysis) = analysis.and_then(|text| serde_json::from_str::<Value>(&text).ok())
        else {
            return Ok(Value::Null);
        };
        let mut hashes = BTreeMap::new();
        let cards: Vec<Card> = analysis["knowledge"]["cards"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| serde_json::from_value::<Card>(v.clone()).ok())
            .filter(|card| {
                (name.is_empty() || card.name == name) && current(tree, &card.source, &mut hashes)
            })
            .take(2)
            .collect();
        let notes = interpretations(root)?;
        let notes: Vec<_> = notes
            .into_iter()
            .filter(|note| {
                note.sources.iter().any(|source| {
                    source.file == file
                        && (name.is_empty()
                            || cards.iter().any(|card| {
                                card.source.line <= source.end_line
                                    && source.line <= card.source.end_line
                            }))
                })
            })
            .filter(|note| {
                note.sources
                    .iter()
                    .all(|source| current(tree, source, &mut hashes))
            })
            .take(2)
            .collect();
        let edges: Vec<_> = cards
            .iter()
            .flat_map(|card| &card.outgoing)
            .filter(|edge| {
                edge.get("source")
                    .and_then(|value| serde_json::from_value::<Source>(value.clone()).ok())
                    .is_some_and(|source| current(tree, &source, &mut hashes))
            })
            .take(6)
            .cloned()
            .collect();
        let notes:Vec<_>=notes.into_iter().map(|note|json!({"id":note.id,"text":note.text,"status":note.status,"origin":note.origin,"sources":note.sources})).collect();
        if edges.is_empty() && notes.is_empty() {
            Ok(Value::Null)
        } else {
            Ok(json!({"candidate_edges":edges,"interpretations":notes,"semantic_proof":false}))
        }
    })();
    read.unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(dir: &Path, file: &str) -> Source {
        let bytes = std::fs::read(dir.join(file)).unwrap();
        let mut hash = Sha256::new();
        hash.update(&bytes);
        Source {
            file: file.into(),
            line: 1,
            end_line: 1,
            sha256: hash.hex_digest(),
        }
    }
    #[test]
    fn changing_any_source_invalidates_the_whole_interpretation() {
        let dir = tempfile::tempdir().unwrap();
        for file in ["a.rs", "b.rs"] {
            std::fs::write(dir.path().join(file), "fn run() {}\n").unwrap();
        }
        store::write_text(dir.path(), r#"{"modules":[]}"#).unwrap();
        let note = Interpretation {
            id: "backup".into(),
            title: "Backup".into(),
            text: "Restores a plan".into(),
            status: "reviewed".into(),
            origin: "fixture-review".into(),
            sources: vec![source(dir.path(), "a.rs"), source(dir.path(), "b.rs")],
        };
        record(dir.path(), &note).unwrap();
        record(dir.path(), &note).unwrap();
        assert_eq!(
            query(dir.path(), "backup", None, 5, 1, false).unwrap().0["interpretations"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        std::fs::write(dir.path().join("b.rs"), "fn changed() {}\n").unwrap();
        let answer = query(dir.path(), "backup", None, 5, 1, false).unwrap().0;
        assert_eq!(answer["interpretations"], json!([]));
        assert_eq!(answer["stale_interpretations"], json!(["backup"]));
        assert!(record(dir.path(), &note).is_err());
    }
    #[test]
    fn sources_outside_the_project_and_invalid_ranges_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut hashes = BTreeMap::new();
        let source = Source {
            file: "../secret".into(),
            line: 1,
            end_line: 1,
            sha256: "a".repeat(64),
        };
        assert!(!current(dir.path(), &source, &mut hashes));
    }
}
