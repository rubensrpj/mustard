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
use crate::domain::text::{CutAt, cut};
use crate::io::project_map::{self as store, open_existing, unreadable};
use crate::io::sha256::Sha256;

pub mod enrichment;
pub mod audit;
pub mod coverage;
pub mod investigation;
pub mod observations;
pub mod relations;
pub(crate) mod catalog;
pub(crate) mod references;
mod navigation;
mod refresh;
pub(crate) mod resources;
pub mod waves;
pub use navigation::Direction;

/// Inventory evidence for search diagnostics, never a completeness guarantee.
/// A contagem lê o mapa sem pôr bloco nenhum em dia: o bloco noutra versão é
/// da passada do scan, e esperar a trava dela seguraria a busca.
pub(crate) fn indexed_file_count(root: &Path) -> Option<u64> {
    let db = store::read_only_existing(&store::model_path(root)).ok()?;
    let count: i64 = db.conn().query_row("SELECT count(*) FROM knowledge_files", [], |row| row.get(0)).ok()?;
    u64::try_from(count).ok()
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_context: Option<RuleContext>,
}

/// Expanded instructions bind interpretation reuse, including nested includes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleContext {
    pub files: Vec<String>,
    pub sha256: String,
}
impl RuleContext {
    pub fn of(root: &Path, files: &[String]) -> Self {
        let files: Vec<_> = files.iter().cloned().collect::<BTreeSet<_>>().into_iter().collect();
        let mut hash = Sha256::new();
        for rule in crate::io::wave_prompt::project_rules(root, &files) {
            hash.update(rule.path.as_bytes()); hash.update(b"\0");
            hash.update(rule.text.as_bytes()); hash.update(b"\0");
        }
        Self { files, sha256: hash.hex_digest() }
    }
    pub fn current(&self, root: &Path) -> bool { *self == Self::of(root, &self.files) }
}
fn rules_current(root: &Path, note: &Interpretation) -> bool {
    note.rule_context.as_ref().is_none_or(|context| context.current(root))
}

fn note_card(note: &Interpretation) -> Card {
    Card {
        identifiers:String::new(),
        id: note.id.clone(),
        name: note.title.clone(),
        documentation: note.text.clone(),
        signature: String::new(),
        syntax:Value::Null,
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
        test_only: false,
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

pub(crate) fn current(
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
    if !rules_current(tree, note) || note
        .sources
        .iter()
        .any(|source| !current(tree, source, &mut hashes))
    {
        return Err(invalid("knowledge-source-changed-or-invalid"));
    }
    let payload = serde_json::to_string(note).map_err(|e| invalid(e.to_string()))?;
    let mut db = open_existing(&store::model_path(root))?;
    let existing: Option<String> = db
        .conn()
        .query_row(
            "SELECT payload FROM knowledge_notes WHERE id=?1",
            [&note.id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| unreadable(e.into()))?;
    if existing.as_deref() == Some(&payload) {
        return Ok(
            json!({"ok":true,"recorded":note.id,"status":note.status,"reused":true,"semantic_validation":"origin-review-only"}),
        );
    }
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
    consistent_query(root, tree, opts, true, None)
}

/// Task-aware retrieval with an optional embedded code channel. This port
/// never selects a paid provider, including projects with legacy AI settings.
pub fn query_for(root: &Path, tree: &Path, opts: &Query<'_>, task: knowledge::investigation::Task<'_>) -> Result<(Value, ProjectMap), MapRefusal> {
    if task.intent.len()>4000 || opts.text.len()>4000 || (opts.symbol.is_some() && !task.intent.trim().is_empty()) {
        return Err(invalid("knowledge-invalid-investigation-context"));
    }
    consistent_query(root, tree, opts, true, Some(task))
}

/// The gateway supplies the inventory allowed by the original tool. Task
/// discovery, current-source reads and graph expansion stay in that inventory.
pub(crate) struct EvidenceScope<'a> {
    pub files: &'a BTreeSet<String>,
    pub seeds: &'a [String],
}

pub(crate) fn cards_for_ids(root: &Path, ids: &BTreeSet<String>) -> Result<Vec<Card>, MapRefusal> {
    catalog::hydrate(open_existing(&store::model_path(root))?.conn(), ids).map_err(unreadable)
}

pub(crate) fn query_for_scope(root: &Path, tree: &Path, opts: &Query<'_>, task: knowledge::investigation::Task<'_>, scope: &EvidenceScope<'_>) -> Result<(Value, ProjectMap), MapRefusal> {
    consistent_scoped_query(root, tree, opts, true, Some(task), Some(scope))
}

/// Compare read generations without exposing the database connection publicly.
pub fn generation(root:&Path)->Result<String,MapRefusal> {
    catalog::ensure_languages(root)?;
    crate::io::map_revision::stamp(open_existing(&store::model_path(root))?.conn()).map_err(unreadable)
}

// Generation evidence must not depend on the interpretation it is about to
// create, otherwise saving one note changes its own cache key on the next run.
fn query_sources(
    root: &Path,
    tree: &Path,
    options: &Query<'_>,
) -> Result<(Value, ProjectMap), MapRefusal> {
    consistent_query(root, tree, options, false, None)
}

/// Source blocks can be read through several database ports. Detect a writer
/// between those reads instead of returning a mixed source generation.
fn consistent_query(root: &Path, tree: &Path, options: &Query<'_>, include_interpretations: bool, task:Option<knowledge::investigation::Task<'_>>) -> Result<(Value,ProjectMap),MapRefusal> {
    consistent_scoped_query(root, tree, options, include_interpretations, task, None)
}

fn consistent_scoped_query(root: &Path, tree: &Path, options: &Query<'_>, include_interpretations: bool, task:Option<knowledge::investigation::Task<'_>>, scope:Option<&EvidenceScope<'_>>) -> Result<(Value,ProjectMap),MapRefusal> {
    for _ in 0..2 {
        let before = generation(root)?;
        let mut result=query_internal(root,tree,options,include_interpretations,task,scope)?;
        let after = generation(root)?;
        if before==after {
            result.0["scan_snapshot"]["consistent_generation"]=json!(true);
            if !options.detail && !options.all && !options.refresh {knowledge::projection::compact(&mut result.0);}
            return Ok(result);
        }
    }
    Err(invalid("knowledge-concurrent-scan; retry after the current scan commits"))
}

/// Quantos caracteres do texto de uma interpretação o resumo mostra antes
/// das reticências; o texto que passa por um caractere só cabe no lugar delas
/// e sai inteiro, e `--detail` traz sempre o texto inteiro.
const NOTE_CHARS: usize = 600;

fn query_internal(root: &Path, tree: &Path, options: &Query<'_>, include_interpretations: bool, task:Option<knowledge::investigation::Task<'_>>, scope:Option<&EvidenceScope<'_>>) -> Result<(Value, ProjectMap), MapRefusal> {
    let Query { text: query, file, limit, depth, all, detail, symbol, direction, refresh } = *options;
    // An explicit investigation may follow current callers of its discovery
    // seeds. Literal lookup still requires an exact symbol for navigation.
    let guided = task.is_some_and(|task| !task.intent.trim().is_empty() && task.purpose != knowledge::investigation::Purpose::Locate);
    if (symbol.is_some() && (!query.trim().is_empty() || file.is_some() || refresh)) || (direction != Direction::Outgoing && symbol.is_none() && scope.is_none() && !guided) {
        return Err(invalid("knowledge-navigation-requires-exact-symbol"));
    }
    let detail = detail || all;
    let mut map = store::read_for(root, store::Need::Summary)?;
    map.skeleton = store::read_for(root, store::Need::Terrain)?.skeleton;
    let state = store::read_state_at(&store::model_path(root))?;
    map.state = serde_json::from_str::<ProjectMap>(&state.json).map_err(|e| invalid(e.to_string()))?.state;
    let mut hashes = BTreeMap::new();
    let notes = if include_interpretations { interpretations(root)?.into_iter().filter(|note| scope.is_none_or(|scope|note.sources.iter().all(|source|scope.files.contains(&source.file)))).collect() } else { vec![] };
    let languages = Languages::of_project(root);
    if refresh {
        return Ok((refresh::report(tree, &notes, query, file, limit, all, detail, &languages, &mut hashes), map));
    }
    let mut resources = resources::retrieve_in(root, tree, options, &mut hashes, scope)?;
    let (fresh, stale): (Vec<_>, Vec<_>) = notes.into_iter().partition(|note| rules_current(tree, note) && note.sources.iter().all(|source| current(tree, source, &mut hashes)));
    let mut index_unavailable = false;
    let exact_name = scope.is_none() && symbol.is_none() && catalog::exact_name(root,options)?;
    let discovery = if scope.is_none() && symbol.is_none() && !exact_name && !query.trim().is_empty() {
        let discovered = if task.is_some() { crate::io::map_search::discovery_native(root,tree,query,&languages) } else { crate::io::map_search::discovery(root,tree,query,&languages) };
        match discovered {
            Ok(discovery) => Some(discovery),
            Err(_) => { index_unavailable = true; None }
        }
    } else { None };
    let mut referenced = references::targets(root,&resources.items)?;
    referenced.retain(|_,source|scope.is_none_or(|scope|scope.files.contains(&source.file)) && current(tree,source,&mut hashes));
    let pool = if let Some(scope)=scope {catalog::scoped_candidates(root,options,scope)?} else {catalog::candidates(root,options,&fresh,discovery.as_ref(),&referenced.keys().cloned().collect::<Vec<_>>(),exact_name)?};
    let outdated_packs=pool.outdated;
    let indexed_symbols = pool.total;
    let hydrated_candidates = pool.cards.len();
    let mut candidates_omitted = pool.omitted;
    let mut cards = pool.cards;
    let max = if all { indexed_symbols } else { limit.clamp(1, 50) };
    // Guarda vagas da resposta para as relações: com todas as vagas tomadas
    // pela semelhança, quem chama a conclusão ou a entrada nunca aparecia.
    let seed_limit = if scope.is_some() {max.saturating_sub(max/3).max(1)}
        else if guided && direction != Direction::Outgoing && depth > 0 {max.saturating_sub((max/3).max(1)).max(1)} else {max};
    // A declaração que a pergunta cita pelo nome entra mesmo quando a
    // semelhança não a trouxe, e com ela os cartões do arquivo dela que casam
    // com a pergunta. Os homônimos do nome contam contra as mesmas vagas.
    let named = if scope.is_none() && symbol.is_none() && !exact_name {catalog::named(root,options,seed_limit)?} else {Vec::new()};
    let missing: BTreeSet<_> = named.iter().filter(|id| !cards.iter().any(|card| &card.id == *id)).cloned().collect();
    if !missing.is_empty() {
        cards.extend(cards_for_ids(root, &missing)?);
    }
    let named_files: BTreeSet<String> = cards.iter().filter(|card| named.contains(&card.id)).map(|card| card.source.file.clone()).collect();
    if !named_files.is_empty() {
        candidates_omitted |= catalog::expand_files(root, &mut cards, &named_files.iter().cloned().collect::<Vec<_>>(), query, all)?.1;
    }
    // A vez do cartão na resposta: 0 a declaração citada pelo nome, 1 outro
    // cartão do arquivo dela, 2 o resto, que segue a ordem por semelhança.
    let named_rank = |card: &Card| u8::from(!named.contains(&card.id)) + u8::from(!named_files.contains(&card.source.file));
    let mut selected = BTreeSet::new();
    let mut order = Vec::new();
    let mut reasons = BTreeMap::new();
    // A declaração citada toma a vaga antes das interpretações e referências,
    // sem tomar a reserva das relações.
    for i in named.iter().filter_map(|id| cards.iter().position(|card| &card.id == id)) {
        if selected.len() < seed_limit && current(tree, &cards[i].source, &mut hashes) && selected.insert(i) {
            order.push(i);
            reasons.insert(i, "exact-name");
        }
    }
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
    if let Some(scope)=scope {
        for (i,_) in cards.iter().enumerate().filter(|(_,card)|scope.seeds.contains(&card.id)) {
            if !ranking.contains(&i) {ranking.push(i);}
        }
    }
    // Os cartões do arquivo da declaração citada que a semelhança não pôs na
    // ordem entram por ela; a investigação lê esses arquivos primeiro.
    if !named.is_empty() {
        let heads = named.iter().filter_map(|id| cards.iter().position(|card| &card.id == id));
        let similar = knowledge::ranked(&cards, query, &languages).into_iter().filter(|&i| named_files.contains(&cards[i].source.file));
        for i in heads.chain(similar).collect::<Vec<_>>() {
            if !ranking.contains(&i) {ranking.push(i);}
        }
        ranking.sort_by_key(|&i| named_rank(&cards[i]));
    }
    let investigation = if let Some(task)=task {
        Some(investigation::prepare(root,tree,options,task,&mut cards,&mut ranking,&mut hashes,scope)?)
    } else {None};
    if let Some(native)=&investigation {candidates_omitted|=native.omitted;}
    // Decision candidates precede the presentation/graph budget. A short
    // declaration must remain available to the selector even when a neighbour
    // would otherwise consume the last visible card. This is internal to the
    // gateway and removed before its final report is emitted.
    let task_candidates:Vec<_>=if scope.is_some() {
        ranking.iter().copied().filter(|&i|current(tree,&cards[i].source,&mut hashes)).take(24).map(|i|cards[i].clone()).collect()
    }else{Vec::new()};
    let rank_trace=if scope.is_some() && std::env::var_os("MUSTARD_SEARCH_TRACE").is_some_and(|value|value=="1") {
        Some(cards.iter().enumerate().map(|(i,card)| {
            let rank=ranking.iter().position(|ranked|*ranked==i).map(|at|at+1);
            let fresh=current(tree,&card.source,&mut hashes);
            let retained=task_candidates.iter().any(|candidate|candidate.id==card.id);
            let mut trace=investigation.as_ref().map_or_else(||json!({}),|native|native.trace(card));
            trace["id"]=json!(card.id);trace["source"]=json!(card.source);trace["rank"]=json!(rank);
            trace["source_current"]=json!(fresh);trace["decision_retained"]=json!(retained);
            trace["decision_status"]=json!(if !fresh {"stale-source"} else if scope.is_some_and(|scope|!scope.files.contains(&card.source.file)) {"outside-request-inventory"}
                else if trace["source_admission"]!="source-read" {"source-not-admitted"} else if rank.is_none() {"no-written-ranking-signal"}
                else if !retained {"decision-reservoir-cut"} else {"retained-for-decision"});
            trace
        }).collect::<Vec<_>>())
    }else{None};
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
    if !named.is_empty() {
        order.sort_by_key(|&i| named_rank(&cards[i]));
    }
    candidates_omitted |= catalog::neighbors_in(root,&mut cards,&order,depth,direction,all,scope)?;
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
    let mut outside_scope_relations = 0;
    let mut ambiguous_relations = 0;
    let primary_ids:BTreeSet<String> = order.iter().map(|&i|cards[i].id.clone()).collect();
    let primary_refs:BTreeSet<&str> = primary_ids.iter().map(String::as_str).collect();
    for i in order {
        let card = &mut cards[i];
        for edges in [&mut card.callers,&mut card.outgoing] {
            edges.retain(|edge| {
                let Some(source)=edge.get("source").and_then(|value|serde_json::from_value::<Source>(value.clone()).ok()) else {
                    stale_relations+=1;
                    return false;
                };
                if scope.is_some_and(|scope|!scope.files.contains(&source.file)) {
                    outside_scope_relations+=1;
                    return false;
                }
                let valid=current(tree,&source,&mut hashes);
                stale_relations+=usize::from(!valid);
                valid
            });
        }
        let outgoing = card.outgoing.len();
        let callers = card.callers.len();
        ambiguous_relations += card.outgoing.iter().chain(&card.callers).filter(|edge| edge["resolution"] == "ambiguous").count();
        let mut item = if detail { serde_json::to_value(&*card).map_err(|e| invalid(e.to_string()))? } else { knowledge::summary(card) };
        compacted_relations += outgoing - item["outgoing"].as_array().map_or(0, Vec::len) + callers - item["callers"].as_array().map_or(0, Vec::len);
        item["retrieval"] = json!(reasons[&i]);
        item["evidence_basis"] = json!({"source":"current-sha256", "selection":reasons[&i], "behavior":"not-proven-by-reference"});
        if let Some(scope)=scope {item["native_search_owner"]=json!(scope.seeds.contains(&card.id));}
        if !detail {
            let witnesses = knowledge::evidence::compact_witnesses(card, query, &languages);
            if !witnesses.is_null() { item["matched_evidence"] = witnesses; }
        }
        if !card.annotations.is_empty() {
            item["annotation_status"] = json!("author-assertion; not semantic proof");
        }
        item["current_outgoing_count"] = json!(outgoing);
        item["current_caller_count"] = json!(callers);
        if let (Some(native),Some(task))=(&investigation,task) {native.adorn(card,&mut item,task.purpose,&primary_refs);}
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
            if !detail && fresh[i].text.chars().count() > NOTE_CHARS + 1 {
                note["text"] = json!(cut(&fresh[i].text, NOTE_CHARS + 1, CutAt::Char));
                note["text_compacted"] = json!(true);
            }
            note
        })
        .collect();
    let flows:Vec<_>=result.iter().filter(|card|card["routes"].as_array().is_some_and(|rows|!rows.is_empty()) || card["outgoing"].as_array().is_some_and(|rows|!rows.is_empty())).map(|card|
        json!({"entry":card["id"],"static_targets":card["outgoing"].as_array().into_iter().flatten().map(|edge|&edge["target"]).collect::<Vec<_>>(),"runtime_order":"unknown"})).collect();
    let capabilities=if detail {knowledge::capabilities::groups(&result)} else {Vec::new()};
    let mut report = json!({"ok":true,"schema_version":knowledge::VERSION,"query":query,"cards":result,"interpretations":interpretations,
        "resources":resources.items,"scan_coverage":coverage::report(root)?,"resource_coverage":{"indexed":resources.available,"files_at_scan":resources.indexed_files,
            "issues_at_scan":resources.issues,"issue_count":resources.issue_count,"issues_compacted":resources.issue_count>resources.issues.len() as i64,"stale_excerpts":resources.stale,"has_more":resources.omitted>0,
            "meaning":"verbatim text only; accepted formats and excluded paths declared by registry"},
        "flows":flows,"capability_candidates":capabilities,"scan_snapshot":{"head":map.state.head,"projects":map.projects,"languages":map.languages,
            "skeleton":map.skeleton.iter().filter(|layer|all || result.iter().any(|card|card["source"]["file"].as_str().is_some_and(|file|file.starts_with(&format!("{}/",layer.dir))))).collect::<Vec<_>>(),
            "freshness":"catalog-at-last-scan; selected sources individually verified"},
        "local_hybrid_index":local_hybrid_index,"projection":if detail {"detail"} else {"summary"},
        "catalog":{"symbols":indexed_symbols,"entries":indexed_symbols,"entry_kinds":"declarations and source-file evidence","hydrated_candidates":hydrated_candidates,"hydrated_with_neighbors":cards.len(),"has_more_candidates":candidates_omitted,"outdated_packs":outdated_packs},
        "navigation":{"symbol":symbol,"direction":direction,"max_depth":depth.min(4),"paths":if symbol.is_some() || guided {walk.paths}else{vec![]},"omitted_destinations":omitted_edges,"stale_destinations":stale_navigation,"stale_relations":stale_relations,"outside_scope_relations":outside_scope_relations,"ambiguous_relations":ambiguous_relations,"impact_completeness":"unknown; static links only"},
        "expand":{"detail":"Repeat the query with --detail; --symbol <id> selects one exact declaration.","source":"run map slice --file <source.file> --name <name>","relations":"run knowledge --symbol <id> --direction callers --detail; both ends checked, static resolution is not runtime proof.","refresh":"run knowledge --refresh to inspect stale interpretations without regenerating them."},
        "stale_interpretations":stale.iter().map(|note|&note.id).collect::<Vec<_>>(),"gaps":gaps,
        "origin":"scan-and-versioned-interpretations","remote_model_calls":0,
        "retrieval_method":if exact_name {"exact-name-index"}else{"native-index-and-vocabulary"},"vectors_enabled":task.is_none() && crate::domain::config::ProjectConfig::load(tree).ai_vectors_enabled(),
        "local_model_calls":if task.is_none() && crate::domain::config::ProjectConfig::load(tree).ai_vectors_enabled(){Value::Null}else{json!(0)}});
    if let Some(native)=investigation {
        if !native.verify(tree) {return Err(invalid("knowledge-source-changing; repeat the investigation"));}
        report["investigation"]=native.report;
        report["retrieval_method"]=json!("native-index-and-current-source");
        report["local_model_calls"]=report["investigation"]["local_model_calls"].clone();
        report["vectors_enabled"]=json!(report["local_model_calls"].as_u64().is_some_and(|calls|calls>0));
    }
    if let Some(scope)=scope {
        report["candidate_flow"]=json!({"native_owners":scope.seeds.len(),"hydrated_candidates":cards.len(),
            "decision_candidates":task_candidates.len(),"decision_candidates_before_presentation":true,
            "native_owners_in_decision":task_candidates.iter().filter(|card|scope.seeds.contains(&card.id)).count(),
            "meaning":"Stage counts describe retrieval and source admission, not semantic correctness."});
        report["task_candidates"]=json!(task_candidates);
        if let Some(trace)=rank_trace {report["retrieval_trace"]=json!({"method":"explicit MUSTARD_SEARCH_TRACE=1; per-channel ranks, source admission and decision status; not a relevance judgement","candidates":trace});}
    }
    Ok((report,map))
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
        let mut notes: Vec<_> = notes
            .into_iter()
            .filter(|note| {
                note.sources.iter().any(|source| {
                    source.file == file
                        && (name.is_empty() || cards.iter().any(|card| card.source.line <= source.end_line && source.line <= card.source.end_line))
                })
            })
            .filter(|note| rules_current(tree, note) && note.sources.iter().all(|source| current(tree, source, &mut hashes)))
            .collect();
        notes.sort_by_key(|note| (note.status != "reviewed",note.id.clone()));
        notes.truncate(2);
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
    #[test]
    fn investigation_reserves_room_for_a_verified_completion_caller_without_enabling_literal_navigation() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();
        std::fs::write(root.join("entry.rs"),"fn completed() { write_export(); }\n").unwrap();
        std::fs::write(root.join("write.rs"),"fn write_export() {}\n").unwrap();
        let mut raw=json!({"modules":[
            {"path":"entry.rs","analysis":{"content_sha256":source(root,"entry.rs").sha256,"parse_complete":true},"declarations":[{"name":"completed","kind":"function","line":1,"end_line":1}]},
            {"path":"write.rs","analysis":{"content_sha256":source(root,"write.rs").sha256,"parse_complete":true},"declarations":[{"name":"write_export","kind":"function","line":1,"end_line":1,
                "used_by":["entry.rs:1:completed"]}]}]});
        knowledge::enrich(&mut raw);store::write_text(root,&raw.to_string()).unwrap();
        let opts=Query{text:"write_export",file:None,limit:2,depth:1,all:false,detail:false,symbol:None,direction:Direction::Both,refresh:false};
        assert!(query_for(root,root,&opts,knowledge::investigation::Task::default()).is_err());
        let task=knowledge::investigation::Task{intent:"Locate the completion caller that writes the export",purpose:knowledge::investigation::Purpose::Spec};
        let report=query_for(root,root,&opts,task).unwrap().0;
        assert!(report["cards"].as_array().unwrap().iter().any(|card|card["name"]=="completed"),"{report}");
        assert!(!report["navigation"]["paths"].as_array().unwrap().is_empty(),"{report}");
        assert!(report["cards"].as_array().unwrap().iter().all(|card|card["evidence_basis"]["source"]=="current-sha256"));
        assert_eq!(report["remote_model_calls"],0);
        std::fs::write(root.join("entry.rs"),"fn changed() {}\n").unwrap();
        let report=query_for(root,root,&opts,task).unwrap().0;
        assert!(!report["cards"].as_array().unwrap().iter().any(|card|card["name"]=="completed"));
    }
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
    fn reviewed_notes_lead_wave_context() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();
        for file in ["a.rs","b.rs"] {std::fs::write(root.join(file),"fn run() {}\n").unwrap();}
        let mut raw=json!({"modules":[{"path":"a.rs","analysis":{"content_sha256":source(root,"a.rs").sha256},
            "declarations":[{"name":"run","kind":"function","line":1,"end_line":1}]}]});
        knowledge::enrich(&mut raw);store::write_text(root,&raw.to_string()).unwrap();
        for (id,status) in [("a-hypothesis","hypothesis"),("b-hypothesis","hypothesis"),("z-reviewed","reviewed")] {
            record(root,&Interpretation{id:id.into(),title:"quartz beacon".into(),text:"Quarzt archive handles the beacon.".into(),status:status.into(),origin:"review-fixture".into(),sources:vec![source(root,"a.rs"),source(root,"b.rs")],rule_context:None}).unwrap();
        }
        assert_eq!(for_source(root,root,"a.rs","")["interpretations"][0]["status"],"reviewed");
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
            rule_context: None,
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

    /// Um projeto em TypeScript, com cada arquivo de `files`, lido pelo scan
    /// compilado junto com os testes.
    fn scanned_typescript(files: &[(&str, &str)]) -> tempfile::TempDir {
        let scan = crate::Scan::locate();
        assert!(scan.is_compiled_alongside(), "rode `cargo build -p scan` antes de `cargo test -p mustard-core`");
        let dir = tempfile::tempdir().unwrap();
        for (file, text) in files {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        scan.scan(dir.path(), &store::model_path(dir.path())).expect("the scan writes the map");
        dir
    }

    /// A pergunta de quem implementa, como `run knowledge --purpose implement`
    /// a faz: os cartões da resposta, cada um como `arquivo:nome`. Com `trace`,
    /// a intenção de uma investigação que segue um nível de quem chama as
    /// declarações achadas.
    fn implement_answer(root: &Path, text: &str, limit: usize, trace: Option<&str>) -> Vec<String> {
        let (depth, direction) = if trace.is_some() { (1, Direction::Callers) } else { (0, Direction::Outgoing) };
        let opts = Query { text, file: None, limit, depth, all: false, detail: false, symbol: None, direction, refresh: false };
        let task = knowledge::investigation::Task { intent: trace.unwrap_or_default(), purpose: knowledge::investigation::Purpose::Implement };
        let report = query_for(root, root, &opts, task).unwrap().0;
        report["cards"].as_array().unwrap().iter().map(|card| format!("{}:{}", card["source"]["file"].as_str().unwrap(), card["name"].as_str().unwrap())).collect()
    }

    const UPLOAD: &str = "/** Retries the upload request after a timeout until the server answers. */\nexport async function retryUpload(request: string): Promise<string> {\n  // retry the upload request after each timeout\n  return request;\n}\n";
    const SENDER: &str = "import { retryUpload } from \"./upload\";\n\nexport async function sendRequest(request: string): Promise<string> {\n  return retryUpload(request);\n}\n";
    const REQUEST: &str = "/** Sends the upload request and retries it after a timeout. */\nexport function sendRequest(request: string): string {\n  // the request retries the upload after a timeout\n  return request;\n}\n";

    #[test]
    fn a_question_naming_a_declaration_returns_its_card_first_and_its_file_next() {
        let dir = scanned_typescript(&[
            ("src/limits.ts", "export const RETRY_LIMIT = 3;\n\n/** Waits between upload retries. */\nexport function nextDelay(attempt: number): number {\n  return attempt * 200;\n}\n"),
            ("src/policy.ts", "export class Backoff {\n  /** Grows the wait between upload retries. */\n  grow(wait: number): number {\n    return wait * 2;\n  }\n}\n"),
            ("src/upload.ts", UPLOAD),
            ("src/request.ts", REQUEST),
        ]);
        for (question, named, next) in [
            ("how many times does the upload retry the request after a timeout? retry_limit", "src/limits.ts:RETRY_LIMIT", "src/limits.ts:nextDelay"),
            ("how long does the Backoff wait before the upload retries the request after a timeout", "src/policy.ts:Backoff", "src/policy.ts:grow"),
        ] {
            let cards = implement_answer(dir.path(), question, 3, None);
            assert_eq!(cards[..2], [named, next], "{question}: {cards:?}");
            let file = named.split_once(':').unwrap().0;
            assert!(cards[2..].iter().all(|card| !card.starts_with(file)), "{question}: {cards:?}");
        }
    }

    #[test]
    fn a_prose_word_or_a_name_with_more_homonyms_than_slots_does_not_lead_the_answer() {
        let mut files = vec![("src/upload.ts", UPLOAD), ("src/values.ts", "export const timeout = 5000;\nexport const Timeout = 6000;\n")];
        let jobs = ["src/jobs/one.ts", "src/jobs/two.ts", "src/jobs/three.ts"];
        files.extend(jobs.map(|file| (file, "export function runJob(): void {}\n")));
        let dir = scanned_typescript(&files);
        let cards = implement_answer(dir.path(), "Timeout. Does runJob retry the upload after a timeout?", 2, None);
        assert_eq!(cards.first().map(String::as_str), Some("src/upload.ts:retryUpload"), "{cards:?}");
    }

    #[test]
    fn a_word_cited_only_by_its_capitals_names_only_the_same_spelling() {
        for (file, text, question) in [
            ("src/policy.ts", "export const BACKOFF = 2;\n", "how does the Backoff retry the upload after a timeout"),
            ("src/client.ts", "export function api(): number {\n  return 1;\n}\n", "how does the API retry the upload after a timeout"),
        ] {
            let dir = scanned_typescript(&[(file, text), ("src/upload.ts", UPLOAD)]);
            let cards = implement_answer(dir.path(), question, 3, None);
            assert_eq!(cards.first().map(String::as_str), Some("src/upload.ts:retryUpload"), "{question}: {cards:?}");
        }
    }

    #[test]
    fn the_spelling_the_question_writes_comes_before_its_case_variants() {
        let dir = scanned_typescript(&[
            ("src/a.ts", "export const uploadTimeout = 5000;\n"),
            ("src/b.ts", "export class UploadTimeout {}\n"),
            ("src/upload.ts", UPLOAD),
        ]);
        let cards = implement_answer(dir.path(), "how does the UploadTimeout stop the upload retry", 3, None);
        assert_eq!(cards[..2], ["src/b.ts:UploadTimeout", "src/a.ts:uploadTimeout"], "{cards:?}");
    }

    #[test]
    fn the_cited_name_with_fewer_homonyms_comes_first() {
        let dir = scanned_typescript(&[
            ("src/jobs/one.ts", "export function runJob(): void {}\n"),
            ("src/jobs/two.ts", "export function runJob(): void {}\n"),
            ("src/upload.ts", UPLOAD),
        ]);
        let cards = implement_answer(dir.path(), "does runJob call retryUpload after a timeout", 3, None);
        assert_eq!(cards[..3], ["src/upload.ts:retryUpload", "src/jobs/one.ts:runJob", "src/jobs/two.ts:runJob"], "{cards:?}");
    }

    #[test]
    fn cited_declarations_leave_the_caller_reserve_of_a_guided_navigation() {
        let dir = scanned_typescript(&[
            ("src/upload.ts", "export function retryUpload(request: string): string {\n  return request;\n}\n"),
            ("src/cancel.ts", "export function cancelUpload(): void {}\n"),
            ("src/pause.ts", "export function pauseUpload(): void {}\n"),
            ("src/send.ts", SENDER),
        ]);
        let cards = implement_answer(dir.path(), "who calls retryUpload, cancelUpload or pauseUpload", 3, Some("Trace who retries the upload"));
        assert_eq!(cards, ["src/upload.ts:retryUpload", "src/cancel.ts:cancelUpload", "src/send.ts:sendRequest"]);
    }

    #[test]
    fn a_cited_name_enters_with_all_its_homonyms_or_leaves_the_slots_to_similarity() {
        let jobs = ["src/jobs/one.ts:runJob", "src/jobs/three.ts:runJob", "src/jobs/two.ts:runJob"];
        let mut files = vec![("src/upload.ts", UPLOAD), ("src/send.ts", SENDER)];
        files.extend(jobs.map(|job| (job.split_once(':').unwrap().0, "export function runJob(): void {}\n")));
        let dir = scanned_typescript(&files);
        let question = "does runJob retry the upload after a timeout";
        assert_eq!(implement_answer(dir.path(), question, 3, None), jobs);
        // A investigação guiada guarda uma das três vagas para quem chama: os
        // três homônimos não cabem nas duas que sobram, e o nome não lidera.
        let traced = implement_answer(dir.path(), question, 3, Some("Trace who retries the upload"));
        assert_eq!(traced.first().map(String::as_str), Some("src/upload.ts:retryUpload"), "{traced:?}");
    }

    #[test]
    fn an_acronym_names_the_declaration_of_its_spelling_even_opening_the_sentence() {
        let dir = scanned_typescript(&[("src/client.ts", "export const API = \"https://uploads.example.com\";\n"), ("src/upload.ts", UPLOAD)]);
        for question in ["how does the API retry the upload after a timeout", "API retries the upload after a timeout?"] {
            let cards = implement_answer(dir.path(), question, 3, None);
            assert_eq!(cards.first().map(String::as_str), Some("src/client.ts:API"), "{question}: {cards:?}");
        }
    }

    /// O resumo mostra 600 caracteres do texto de uma interpretação antes das
    /// reticências: o de 601 sai inteiro e não se diz compactado; o de 602
    /// fica com os 600 primeiros e se diz compactado.
    #[test]
    fn the_summary_keeps_600_characters_of_a_note_before_the_ellipsis() {
        for (chars, shown, flagged) in [(601, "x".repeat(601), Value::Null), (602, format!("{}…", "x".repeat(600)), json!(true))] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("a.rs"), "fn run() {}\n").unwrap();
            store::write_text(dir.path(), r#"{"modules":[]}"#).unwrap();
            let note = Interpretation {
                id: "backup".into(),
                title: "Backup".into(),
                text: "x".repeat(chars),
                status: "reviewed".into(),
                origin: "fixture-review".into(),
                rule_context: None,
                sources: vec![source(dir.path(), "a.rs")],
            };
            record(dir.path(), &note).unwrap();
            let answer = query(dir.path(), "backup", None, 5, 1, false).unwrap().0;
            assert_eq!(answer["interpretations"][0]["text"], json!(shown), "{chars}");
            assert_eq!(answer["interpretations"][0]["text_compacted"], flagged, "{chars}");
        }
    }
}
