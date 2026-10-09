//! Native knowledge retrieval and versioned, multi-source interpretations.
//! Source freshness is necessary, but never a semantic proof of the claim.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::domain::knowledge::{self, Card, Source};
use crate::domain::normalize::Languages;
use crate::domain::project_map::{MapRefusal, ProjectMap};
use crate::io::project_map::{self as store, open_existing, unreadable};
use crate::io::sha256::Sha256;

pub mod enrichment;
pub mod audit;
pub(crate) mod catalog;
pub(crate) mod references;
mod navigation;
mod refresh;
pub(crate) mod resources;
pub use navigation::Direction;

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

fn note_card(note: &Interpretation) -> Card {
    Card {
        identifiers:String::new(),
        id: note.id.clone(),
        name: note.title.clone(),
        documentation: note.text.clone(),
        signature: String::new(),
        body_comment: String::new(),
        kind: "interpretation".into(),
        source: note.sources[0].clone(),
        literals: vec![],
        file_documentation: String::new(),
        annotations: vec![],
        parse_complete: None,
        contracts: vec![],
        routes: vec![],
        tests: vec![],
        inline_tests: false,
        outgoing: vec![],
        callers: vec![],
        unresolved_calls: 0,
    }
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
        let note: Interpretation =
            serde_json::from_str(&text).map_err(|e| invalid(e.to_string()))?;
        if note.sources.is_empty() || note.sources.len() > 32 {
            return Err(invalid("knowledge-invalid-interpretation-sources"));
        }
        Ok(note)
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
        crate::io::map_revision::bump(tx)?;
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
    query_with(
        root,
        tree,
        &Query {
            text: query,
            file,
            limit,
            depth,
            all,
            detail: all,
            symbol: None,
            direction: Direction::Outgoing,
            refresh: false,
        },
    )
}

pub struct Query<'a> {
    pub text: &'a str,
    pub file: Option<&'a str>,
    pub limit: usize,
    pub depth: usize,
    pub all: bool,
    pub detail: bool,
    pub symbol: Option<&'a str>,
    pub direction: Direction,
    pub refresh: bool,
}

pub fn query_with(
    root: &Path,
    tree: &Path,
    opts: &Query<'_>,
) -> Result<(Value, ProjectMap), MapRefusal> {
    consistent_query(root, tree, opts, true)
}

// Generation evidence must not depend on the interpretation it is about to
// create, otherwise saving one note changes its own cache key on the next run.
fn query_sources(
    root: &Path,
    tree: &Path,
    options: &Query<'_>,
) -> Result<(Value, ProjectMap), MapRefusal> {
    consistent_query(root, tree, options, false)
}

/// Source blocks can be read through several database ports. Detect a writer
/// between those reads instead of returning a mixed source generation.
fn consistent_query(root: &Path, tree: &Path, options: &Query<'_>, include_interpretations: bool) -> Result<(Value,ProjectMap),MapRefusal> {
    for _ in 0..2 {
        let before = crate::io::map_revision::stamp(open_existing(&store::model_path(root))?.conn()).map_err(unreadable)?;
        let mut result=query_internal(root,tree,options,include_interpretations)?;
        let after = crate::io::map_revision::stamp(open_existing(&store::model_path(root))?.conn()).map_err(unreadable)?;
        if before==after {result.0["scan_snapshot"]["consistent_generation"]=json!(true); return Ok(result);}
    }
    Err(invalid("knowledge-concurrent-scan; retry after the current scan commits"))
}

fn query_internal(root: &Path, tree: &Path, options: &Query<'_>, include_interpretations: bool) -> Result<(Value, ProjectMap), MapRefusal> {
    let Query { text: query, file, limit, depth, all, detail, symbol, direction, refresh } = *options;
    if (symbol.is_some() && (!query.trim().is_empty() || file.is_some() || refresh)) || (direction != Direction::Outgoing && symbol.is_none()) {
        return Err(invalid("knowledge-navigation-requires-exact-symbol"));
    }
    let detail = detail || all;
    let mut map = store::read_for(root, store::Need::Summary)?;
    map.skeleton = store::read_for(root, store::Need::Terrain)?.skeleton;
    let state = store::read_state_at(&store::model_path(root))?;
    map.state = serde_json::from_str::<ProjectMap>(&state.json).map_err(|e| invalid(e.to_string()))?.state;
    let mut hashes = BTreeMap::new();
    let notes = if include_interpretations { interpretations(root)? } else { vec![] };
    let languages = Languages::of_project(root);
    if refresh {
        return Ok((refresh::report(tree, &notes, query, file, limit, all, detail, &languages, &mut hashes), map));
    }
    let mut resources = resources::retrieve(root, tree, options, &mut hashes)?;
    let (fresh, stale): (Vec<_>, Vec<_>) = notes.into_iter().partition(|note| note.sources.iter().all(|source| current(tree, source, &mut hashes)));
    let mut index_unavailable = false;
    let exact_name = symbol.is_none() && catalog::exact_name(root,options)?;
    let discovery = if symbol.is_none() && !exact_name && !query.trim().is_empty() {
        match crate::io::map_search::discovery(root, tree, query, &languages) {
            Ok(discovery) => Some(discovery),
            Err(_) => { index_unavailable = true; None }
        }
    } else { None };
    let mut referenced = references::targets(root,&resources.items)?;
    referenced.retain(|_,source|current(tree,source,&mut hashes));
    let pool = catalog::candidates(root,options,&fresh,discovery.as_ref(),&referenced.keys().cloned().collect::<Vec<_>>(),exact_name)?;
    let outdated_packs=pool.outdated;
    let indexed_symbols = pool.total;
    let hydrated_candidates = pool.cards.len();
    let mut candidates_omitted = pool.omitted;
    let mut cards = pool.cards;
    let mut selected = BTreeSet::new();
    let mut order = Vec::new();
    let mut reasons = BTreeMap::new();
    let max = if all { indexed_symbols } else { limit.clamp(1, 50) };
    let note_documents: Vec<Card> = fresh.iter().map(note_card).collect();
    let matching: Vec<_> = knowledge::ranked(&note_documents, query, &languages)
        .into_iter()
        .filter(|i| {
            symbol.is_none()
                && knowledge::interpretation_matches(&note_documents[*i], query, &languages)
                && file.is_none_or(|file| fresh[*i].sources.iter().any(|source| source.file == file))
        })
        .take(if all { usize::MAX } else { max })
        .collect();
    // Business interpretations lead back to all of their source functions.
    for &i in &matching {
        for source in &fresh[i].sources {
            for (n, card) in cards.iter().enumerate().filter(|(_, card)| {
                card.source.file == source.file
                    && file.is_none_or(|file| card.source.file == file)
                    && card.source.line <= source.end_line
                    && source.line <= card.source.end_line
            }) {
                if selected.len() < max && current(tree, &card.source, &mut hashes) && selected.insert(n) {
                    order.push(n);
                    reasons.insert(n, "interpretation-source");
                }
            }
        }
    }
    for (i,card) in cards.iter().enumerate() {
        if selected.len()<max && referenced.get(&card.id)==Some(&card.source) && file.is_none_or(|file|card.source.file==file)
            && current(tree,&card.source,&mut hashes) && selected.insert(i) {
            order.push(i); reasons.insert(i,"explicit-resource-reference");
        }
    }
    let mut stale_cards = 0;
    if let Some(id) = symbol
        && let Some(i) = cards.iter().position(|card| card.id == id)
    {
        if current(tree, &cards[i].source, &mut hashes) {
            selected.insert(i);
            order.push(i);
            reasons.insert(i, "exact-symbol");
        } else {
            stale_cards += 1;
        }
    }
    let seed_limit = max;
    let mut ranking = Vec::new();
    let mut local_hybrid_index = false;
    if let Some(discovery) = discovery {
        let by_place: BTreeMap<_, _> = cards.iter().enumerate().map(|(i, card)| ((card.source.file.clone(), card.source.line, card.name.clone()), i)).collect();
        let indexed: Vec<_> = discovery.places.iter().filter_map(|place| by_place.get(place).copied()).collect();
        let mut ranked = BTreeSet::new();
        // Exact spelling precedes case-folded references (a type and its
        // injected field may otherwise compete). Preserve all card identities.
        for exact_case in [true, false] {
            for (i, card) in cards.iter().enumerate() {
                if (if exact_case { card.name == query.trim() } else { card.name.eq_ignore_ascii_case(query.trim()) }) && ranked.insert(i) {
                    ranking.push(i);
                }
            }
        }
        for (i,card) in cards.iter().enumerate() {
            if knowledge::source_file_matches(card,query,&languages) && ranked.insert(i) {
                ranking.push(i);
            }
        }
        let indexed_files: BTreeSet<_> = indexed.iter().map(|&i| cards[i].source.file.as_str()).collect();
        let weights=catalog::intent_weights(root,query,&languages)?;
        let intent = knowledge::intent_cards_with_weights(&cards, query, &languages,Some(&weights));
        let mut purpose = BTreeMap::new();
        let mut purpose_files = Vec::new();
        let mut independently_supported = BTreeSet::new();
        for candidate in intent {
            let i = candidate.card;
            if candidate.independent {
                independently_supported.insert(i);
            }
            if !purpose.contains_key(&cards[i].source.file) {
                purpose.insert(cards[i].source.file.clone(), i);
                purpose_files.push(cards[i].source.file.clone());
            }
        }
        // Alternate intent and hybrid discovery, retaining the winning symbol
        // within each file instead of substituting its first indexed member.
        for at in 0..purpose_files.len().max(discovery.files.len()) {
            for path in [purpose_files.get(at), discovery.files.get(at)].into_iter().flatten() {
                let winner = purpose.get(path).copied().or_else(|| indexed.iter().find(|&&i| cards[i].source.file == *path).copied());
                if file.is_none_or(|file| file == path)
                    && let Some(i) = winner
                    && (indexed_files.contains(path.as_str()) || independently_supported.contains(&i))
                    && ranked.insert(i)
                {
                    ranking.push(i);
                }
            }
        }
        // One entry per file first; supplementary symbols follow. A large
        // file's many matching declarations cannot crowd out other entries.
        let mut seen_files: BTreeSet<_> = ranking.iter().map(|&i| cards[i].source.file.clone()).collect();
        for &i in &indexed {
            if seen_files.insert(cards[i].source.file.clone()) && ranked.insert(i) {
                ranking.push(i);
            }
        }
        for i in indexed {
            if ranked.insert(i) {
                ranking.push(i);
            }
        }
        local_hybrid_index = !ranking.is_empty();
    }
    if symbol.is_none() && !local_hybrid_index && selected.len() < seed_limit {
        ranking = knowledge::ranked(&cards, query, &languages);
        if exact_name {ranking.sort_by_key(|&i|if cards[i].name==query.trim(){0}else{1});}
    }
    for i in ranking.into_iter().filter(|i| file.is_none_or(|file| cards[*i].source.file == file)) {
        if selected.len() >= seed_limit {
            break;
        }
        if current(tree, &cards[i].source, &mut hashes) {
            if selected.insert(i) {
                order.push(i);
                reasons.insert(i, if exact_name {"exact-name"} else if local_hybrid_index { "native-index-and-vocabulary" } else { "lexical" });
            }
        } else {
            stale_cards += 1;
        }
    }
    candidates_omitted |= catalog::neighbors(root,&mut cards,&order,depth,direction,all)?;
    let walk = navigation::walk(tree, &cards, &order, max, depth, direction, &mut hashes);
    let omitted_edges = walk.omitted;
    let stale_navigation = walk.stale;
    for &i in &walk.order {
        reasons.insert(i, if direction == Direction::Callers { "static-caller" } else { "static-relation" });
    }
    order.extend(walk.order);
    if symbol.is_some() {
        let selected_cards: Vec<_>=order.iter().map(|&i|cards[i].clone()).collect();
        resources.items.extend(references::linked_documents(root,tree,&selected_cards,&mut hashes,detail)?);
    }
    let mut result = Vec::new();
    let mut compacted_relations = 0;
    let mut stale_relations = 0;
    let mut ambiguous_relations = 0;
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
        ambiguous_relations += card.outgoing.iter().chain(&card.callers).filter(|edge| edge["resolution"] == "ambiguous").count();
        let mut item = if detail { serde_json::to_value(&*card).map_err(|e| invalid(e.to_string()))? } else { knowledge::summary(card) };
        compacted_relations += outgoing - item["outgoing"].as_array().map_or(0, Vec::len) + callers - item["callers"].as_array().map_or(0, Vec::len);
        item["retrieval"] = json!(reasons[&i]);
        if !detail {
            let witnesses = knowledge::evidence::compact_witnesses(card, query, &languages);
            if !witnesses.is_null() { item["matched_evidence"] = witnesses; }
        }
        if !card.annotations.is_empty() {
            item["annotation_status"] = json!("author-assertion; not semantic proof");
        }
        item["current_outgoing_count"] = json!(outgoing);
        item["current_caller_count"] = json!(callers);
        result.push(item);
    }
    let mut gaps = vec!["Static relations do not prove runtime order, authorization, business rules or test coverage.".to_string()];
    if candidates_omitted { gaps.push("Some index candidates were not hydrated; narrow the query, navigate an exact symbol or use --all for exhaustive matching evidence.".into()); }
    if outdated_packs>0 {gaps.push(format!("{outdated_packs} evidence packs use an earlier or missing version; run scan to refresh the catalogue."));}
    if index_unavailable {
        gaps.push("Local discovery index unavailable; lexical fallback used. Refresh scan for full retrieval.".into());
    }
    if ambiguous_relations > 0 {
        gaps.push(format!("{ambiguous_relations} ambiguous static relations remain candidates; they do not expand the initial graph. Use --detail to inspect possible targets."));
    }
    if !stale.is_empty() {
        gaps.push(format!("{} interpretations excluded: at least one source changed or disappeared.", stale.len()));
    }
    if stale_cards > 0 {
        gaps.push(format!("{stale_cards} matching declarations excluded: run scan to refresh changed sources."));
    }
    if stale_relations > 0 {
        gaps.push(format!(
            "{stale_relations} static relations excluded: caller or target source changed, disappeared or is invalid; absence is not proof of no consumers."
        ));
    }
    if stale_navigation > 0 {
        gaps.push(format!("{stale_navigation} graph destinations excluded: their sources changed; refresh scan before assessing impact."));
    }
    if omitted_edges > 0 {
        gaps.push(format!("{omitted_edges} related declarations omitted by scope/depth/output or source validity; narrow the query or export --all."));
    }
    if compacted_relations > 0 {
        gaps.push(format!("{compacted_relations} current relations compacted from the response; use --file and --detail to expand selected symbols, or --all for all matching symbols."));
    }
    if result.iter().any(|card| card["parse_complete"] != true) {
        gaps.push("Some selected sources have partial or unknown parse coverage.".into());
    }
    if result.iter().any(|card| card["unresolved_calls"].as_u64().unwrap_or(0) > 0) {
        gaps.push("Some common-name calls were not resolved by the scanner.".into());
    }
    if result.is_empty() && resources.items.is_empty() {
        gaps.push("No current evidence found. Use exact search or scan; absence is not proof that the capability does not exist.".into());
    }
    if !resources.available {
        gaps.push("Documentation/configuration/schema-text coverage is not indexed by this scan; refresh scan to include accepted resources.".into());
    }
    if resources.stale > 0 {
        gaps.push(format!("{} matching resource excerpts excluded: their sources changed or disappeared; refresh scan.", resources.stale));
    }
    let interpretations: Vec<_> = matching
        .into_iter()
        .map(|i| {
            let mut note = json!(fresh[i]);
            if !detail && fresh[i].text.chars().count() > 600 {
                note["text"] = json!(format!("{}…", fresh[i].text.chars().take(600).collect::<String>()));
                note["text_compacted"] = json!(true);
            }
            note
        })
        .collect();
    let flows:Vec<_>=result.iter().filter(|card|card["routes"].as_array().is_some_and(|rows|!rows.is_empty()) || card["outgoing"].as_array().is_some_and(|rows|!rows.is_empty())).map(|card|
        json!({"entry":card["id"],"static_targets":card["outgoing"].as_array().into_iter().flatten().map(|edge|&edge["target"]).collect::<Vec<_>>(),"runtime_order":"unknown"})).collect();
    let capabilities=if detail {knowledge::capabilities::groups(&result)} else {Vec::new()};
    Ok((
        json!({"ok":true,"schema_version":knowledge::VERSION,"query":query,"cards":result,"interpretations":interpretations,
        "resources":resources.items,"resource_coverage":{"indexed":resources.available,"files_at_scan":resources.indexed_files,
            "issues_at_scan":resources.issues,"issue_count":resources.issue_count,"issues_compacted":resources.issue_count>resources.issues.len() as i64,"stale_excerpts":resources.stale,"has_more":resources.omitted>0,
            "meaning":"verbatim text only; accepted formats and excluded paths declared by registry"},
        "flows":flows,"capability_candidates":capabilities,"scan_snapshot":{"head":map.state.head,"projects":map.projects,"languages":map.languages,
            "skeleton":map.skeleton.iter().filter(|layer|all || result.iter().any(|card|card["source"]["file"].as_str().is_some_and(|file|file.starts_with(&format!("{}/",layer.dir))))).collect::<Vec<_>>(),
            "freshness":"catalog-at-last-scan; selected sources individually verified"},
        "local_hybrid_index":local_hybrid_index,"projection":if detail {"detail"} else {"summary"},
        "catalog":{"symbols":indexed_symbols,"entries":indexed_symbols,"entry_kinds":"declarations and source-file evidence","hydrated_candidates":hydrated_candidates,"hydrated_with_neighbors":cards.len(),"has_more_candidates":candidates_omitted,"outdated_packs":outdated_packs},
        "navigation":{"symbol":symbol,"direction":direction,"max_depth":depth.min(4),"paths":if symbol.is_some(){walk.paths}else{vec![]},"omitted_destinations":omitted_edges,"stale_destinations":stale_navigation,"impact_completeness":"unknown; static links only"},
        "expand":{"detail":"Repeat the query with --detail; --symbol <id> selects one exact declaration.","source":"run map slice --file <source.file> --name <name>","relations":"run knowledge --symbol <id> --direction callers --detail; both ends checked, static resolution is not runtime proof.","refresh":"run knowledge --refresh to inspect stale interpretations without regenerating them."},
        "stale_interpretations":stale.iter().map(|note|&note.id).collect::<Vec<_>>(),"gaps":gaps,
        "origin":"scan-and-versioned-interpretations","remote_model_calls":0,
        "retrieval_method":if exact_name {"exact-name-index"}else{"native-index-and-vocabulary"},"vectors_enabled":crate::domain::config::ProjectConfig::load(tree).ai_vectors_enabled(),
        "local_model_calls":if crate::domain::config::ProjectConfig::load(tree).ai_vectors_enabled(){Value::Null}else{json!(0)}}),
        map,
    ))
}

/// Narrow query for a prepared component: no full project deserialization or
/// graph ranking. All interpretation sources are checked in the wave copy.
pub fn for_source(root: &Path, tree: &Path, file: &str, name: &str) -> Value {
    let read = (|| -> Result<Value, MapRefusal> {
        let db = open_existing(&store::model_path(root))?;
        let analysis: Option<String> =
            db.conn().query_row("SELECT analysis FROM texts WHERE path=?1", [file], |row| row.get(0)).optional().map_err(|e| unreadable(e.into()))?.flatten();
        let Some(analysis) = analysis.and_then(|text| serde_json::from_str::<Value>(&text).ok()) else {
            return resources::for_source(root, tree, file, name);
        };
        let mut hashes = BTreeMap::new();
        let cards: Vec<Card> = analysis["knowledge"]["cards"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| serde_json::from_value::<Card>(v.clone()).ok())
            .filter(|card| (name.is_empty() || card.name == name) && current(tree, &card.source, &mut hashes))
            .take(2)
            .collect();
        let notes = interpretations(root)?;
        let notes: Vec<_> = notes
            .into_iter()
            .filter(|note| {
                note.sources.iter().any(|source| {
                    source.file == file
                        && (name.is_empty() || cards.iter().any(|card| card.source.line <= source.end_line && source.line <= card.source.end_line))
                })
            })
            .filter(|note| note.sources.iter().all(|source| current(tree, source, &mut hashes)))
            .take(2)
            .collect();
        let documents = references::linked_documents(root,tree,&cards,&mut hashes,false)?;
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
        let annotations: Vec<_> = cards
            .iter()
            .filter(|card| !card.annotations.is_empty())
            .map(|card| json!({"symbol":card.id,"annotations":card.annotations,"source":card.source}))
            .collect();
        if edges.is_empty() && notes.is_empty() && annotations.is_empty() && documents.is_empty() {
            Ok(Value::Null)
        } else {
            // Fingerprint full evidence before projection: even an edit past
            // the visible excerpt invalidates a prepared component's cache.
            let mut version = Sha256::new();
            version.update(json!({"edges":edges,"notes":notes,"annotations":annotations,"documents":documents}).to_string().as_bytes());
            let annotations: Vec<_>=cards.iter().filter(|card|!card.annotations.is_empty())
            .map(|card|{
                let projection=knowledge::summary(card);json!({"symbol":card.id,"annotations":projection["annotations"],"compacted":projection["annotations_compacted"],"count":card.annotations.len(),"status":"author-assertion"})
            }).collect();
            let edge_count = edges.len();
            let edges: Vec<_> =
                edges.iter().map(|edge| json!({"target":edge["target"],"call_line":edge["call_line"],"resolution":edge["resolution"]})).collect();
            let notes: Vec<_> = notes
                .iter()
                .map(|note| {
                    json!({"id":note.id,"title":note.title,
                "text":note.text.chars().take(500).collect::<String>(),"text_compacted":note.text.chars().count()>500,
                "status":note.status,"origin":note.origin,"source_count":note.sources.len()})
                })
                .collect();
            Ok(json!({"candidate_edges":edges,"interpretations":notes,"annotations": annotations,"documents":documents,"semantic_proof":false,
                "evidence_version": version.hex_digest(),"current_edge_count": edge_count,
                "expand":"run knowledge --file <source file> --detail"}))
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
