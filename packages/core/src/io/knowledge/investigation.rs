//! Native investigation joins indexed structure with current source occurrences.
//! A live hit outside a current pack remains a line hit, never an invented symbol.
use super::{Query, catalog, current, invalid, source_bytes};
use crate::domain::knowledge::{
    self, Card, Source,
    investigation::{Matcher, Purpose, Task},
};
use crate::domain::normalize::Languages;
use crate::domain::project_map::MapRefusal;
use crate::io::sha256::Sha256;
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use std::time::{Duration, Instant};

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LIVE_FILES: usize = 96;
const MAX_LIVE_BYTES: usize = 12 * 1024 * 1024;
const LIVE_TIME: Duration = Duration::from_secs(2);

pub struct Occurrence<'a> {
    pub file: &'a str,
    pub line: u64,
    pub text: &'a str,
}

/// Add current indexed structure to occurrences supplied by an executed tool.
/// The occurrences themselves remain the tool's result, in their original order.
pub fn cross_hits(root: &Path, tree: &Path, hits: &[Occurrence<'_>]) -> Result<Value, MapRefusal> {
    let before = super::generation(root)?;
    let db = super::open_existing(&super::store::model_path(root))?;
    let registry = knowledge::resources::Registry::load().map_err(invalid)?;
    let mut files = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let mut matched = BTreeMap::<String, Vec<u64>>::new();
    let mut unmatched = 0;
    let mut processed = 0;
    let mut bytes = 0;
    let started = Instant::now();
    for hit in hits.iter().take(256) {
        if started.elapsed() >= LIVE_TIME {
            break;
        }
        if !files.contains_key(hit.file) {
            if files.len() >= MAX_LIVE_FILES || bytes >= MAX_LIVE_BYTES {
                break;
            }
            let text = safe_read(tree, hit.file, &registry)
                .ok_or_else(|| invalid("knowledge-live-source-unavailable"))?;
            bytes += text.len();
            files.insert(hit.file.to_string(), (hash(text.as_bytes()), text));
        }
        let Some((digest, text)) = files.get(hit.file) else {
            continue;
        };
        if hit.line == 0 || text.lines().nth(hit.line.saturating_sub(1) as usize) != Some(hit.text)
        {
            return Err(invalid("knowledge-executed-hit-changed"));
        }
        let line =
            i64::try_from(hit.line).map_err(|_| invalid("knowledge-occurrence-line-invalid"))?;
        let id:Option<String>=db.conn().query_row(
            "SELECT id FROM knowledge_symbols WHERE path=?1 AND line<=?2 AND end_line>=?2 AND sha256=?3 ORDER BY end_line-line,line DESC LIMIT 1",
            rusqlite::params![hit.file,line,digest],|row|row.get(0)).optional().map_err(|err|super::unreadable(err.into()))?;
        if let Some(id) = id {
            ids.insert(id.clone());
            matched.entry(id).or_default().push(hit.line);
        } else {
            unmatched += 1;
        }
        processed += 1;
    }
    let cards = catalog::hydrate(db.conn(), &ids).map_err(super::unreadable)?;
    let total = cards.len();
    let evidence:Vec<_>=cards.into_iter().take(4).map(|card|json!({"id":card.id,"name":card.name,"source":card.source,
        "signature":card.signature,"matched_lines":matched.get(&card.id),"status":"current-static-owner",
        "expand":{"command":"mustard-rt","args":["run","knowledge","--symbol",card.id,"--purpose","implement"]}})).collect();
    if before != super::generation(root)?
        || !files.iter().all(|(path, (digest, _))| {
            source_bytes(tree, path).is_some_and(|bytes| hash(&bytes) == *digest)
        })
    {
        return Err(invalid("knowledge-live-evidence-changed"));
    }
    Ok(
        json!({"symbols":evidence,"omitted_symbols":total.saturating_sub(4),"unmapped_occurrences":unmatched,
        "omitted_occurrences":hits.len().saturating_sub(processed),"semantic_completeness":"unknown","local_model_calls":0,"remote_model_calls":0}),
    )
}

pub(super) struct File {
    text: String,
    hash: String,
    hits: Vec<(u64, BTreeSet<usize>)>,
}

#[derive(Default)]
pub(super) struct Investigation {
    files: BTreeMap<String, File>,
    pub report: Value,
    pub omitted: bool,
    alternatives: BTreeMap<String, Vec<Card>>,
}

fn hash(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    hash.hex_digest()
}

fn scoped(file: &str, scope: Option<&str>) -> bool {
    scope.is_none_or(|scope| scope == file)
}

fn safe_read(tree: &Path, path: &str, registry: &knowledge::resources::Registry) -> Option<String> {
    if !registry.admits_path(path)
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return None;
    }
    if !std::fs::metadata(tree.join(path))
        .is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_FILE_BYTES)
    {
        return None;
    }
    let text = String::from_utf8(source_bytes(tree, path)?).ok()?;
    (!text.contains('\0') && !registry.sensitive(&text)).then_some(text)
}

pub(super) fn prepare(
    root: &Path,
    tree: &Path,
    query: &Query<'_>,
    task: Task<'_>,
    cards: &mut Vec<Card>,
    ranking: &mut Vec<usize>,
    hashes: &mut BTreeMap<String, Option<(String, u64)>>,
) -> Result<Investigation, MapRefusal> {
    let languages = Languages::of_project(root);
    let phrase = if task.intent.trim().is_empty() {
        query.text.to_string()
    } else {
        format!("{} {}", query.text, task.intent)
    };
    let mut matcher = Matcher::new(&phrase, &languages).map_err(invalid)?;
    let mut focus = Matcher::new(
        if task.intent.trim().is_empty() {
            query.text
        } else {
            task.intent
        },
        &languages,
    )
    .map_err(invalid)?;
    let registry = knowledge::resources::Registry::load().map_err(invalid)?;
    let mut phases = vec![json!({"operation":"indexed-discovery","candidates":cards.len()})];
    let mut intent_omitted = false;
    if !task.intent.trim().is_empty() && query.symbol.is_none() && !query.all {
        // A separate reservoir preserves original query candidates when the
        // task's vocabulary differs. Intent does not rewrite an exact search.
        let intent = Query {
            text: task.intent,
            ..*query
        };
        let pool = catalog::candidates(root, &intent, &[], None, &[], false)?;
        intent_omitted = pool.omitted;
        let mut ids: BTreeSet<_> = cards.iter().map(|card| card.id.clone()).collect();
        let extra: Vec<_> = pool
            .cards
            .into_iter()
            .filter(|card| ids.insert(card.id.clone()))
            .collect();
        let count = extra.len();
        cards.extend(extra);
        for i in knowledge::ranked(cards, task.intent, &languages) {
            if !ranking.contains(&i) {
                ranking.push(i);
            }
        }
        phases.push(
            json!({"operation":"intent-discovery","added_candidates":count,"partial":pool.omitted}),
        );
    }
    let mut paths = Vec::new();
    let mut seen = BTreeSet::new();
    for &i in ranking.iter() {
        if scoped(&cards[i].source.file, query.file) && seen.insert(cards[i].source.file.clone()) {
            paths.push(cards[i].source.file.clone());
        }
        if paths.len() >= query.limit.clamp(1, 16) {
            break;
        }
    }
    if let Some(symbol) = query.symbol
        && let Some(card) = cards.iter().find(|card| card.id == symbol)
    {
        paths.insert(0, card.source.file.clone());
    }
    if let Some(file) = query.file
        && !paths.iter().any(|path| path == file)
    {
        paths.insert(0, file.into());
    }
    let (expanded, mut omitted) = if query.symbol.is_none() {
        catalog::expand_files(root, cards, &paths, &phrase, query.all)?
    } else {
        (0, false)
    };
    omitted |= intent_omitted;
    phases.push(json!({"operation":"file-to-symbol-expansion","added_candidates":expanded,"partial":omitted}));
    // Include current modifications and untracked, nonignored files even if
    // the index has no corresponding symbol. Never modify the user's index.
    let changes = crate::platform::git::run(
        tree,
        &[
            "ls-files",
            "--modified",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    );
    if changes.ok {
        for path in changes
            .stdout
            .split('\0')
            .filter(|path| !path.is_empty() && scoped(path, query.file))
        {
            if !paths.iter().any(|old| old == path) {
                paths.push(path.into());
            }
        }
    }
    // With no indexed destination, perform an explicit source fallback. Its
    // scope and truncation are visible; zero hits never proves no capability.
    let fallback = ranking.is_empty() && query.symbol.is_none() && !matcher.slots.is_empty();
    let mut repository_listing = false;
    if fallback {
        let listed =
            crate::platform::git::run(tree, &["ls-files", "-co", "--exclude-standard", "-z"]);
        repository_listing = listed.ok;
        if listed.ok {
            for path in listed
                .stdout
                .split('\0')
                .filter(|path| !path.is_empty() && scoped(path, query.file))
            {
                if !paths.iter().any(|old| old == path) {
                    paths.push(path.into());
                }
            }
        }
    }
    let started = Instant::now();
    let mut files = BTreeMap::new();
    let mut bytes = 0;
    let mut skipped = 0;
    for path in &paths {
        if files.len() >= MAX_LIVE_FILES
            || bytes >= MAX_LIVE_BYTES
            || started.elapsed() >= LIVE_TIME
        {
            omitted = true;
            break;
        }
        let Some(text) = safe_read(tree, path, &registry) else {
            skipped += 1;
            continue;
        };
        bytes += text.len();
        let mut hits = Vec::new();
        for (at, line) in text.lines().enumerate() {
            if matcher.might_match(line) {
                let matched = matcher.matched(line);
                if !matched.is_empty() {
                    hits.push((at as u64 + 1, matched));
                }
            }
        }
        files.insert(
            path.clone(),
            File {
                hash: hash(text.as_bytes()),
                text,
                hits,
            },
        );
    }
    omitted |= skipped > 0 || (fallback && !repository_listing);
    let mut alternatives = BTreeMap::new();
    let mut promotions = 0;
    let mut ranked_paths = BTreeSet::new();
    let mut own = BTreeMap::new();
    for (i, card) in cards.iter().enumerate() {
        if files.contains_key(&card.source.file)
            && scoped(&card.source.file, query.file)
            && current(tree, &card.source, hashes)
        {
            let slots = focus.matched(&knowledge::evidence::own_text(card));
            own.insert(i, slots.len());
        }
    }
    for i in ranking.iter_mut() {
        let path = cards[*i].source.file.clone();
        if !ranked_paths.insert(path.clone()) {
            continue;
        }
        let Some(file) = files.get(&path) else {
            continue;
        };
        let mut choices: Vec<_> = cards
            .iter()
            .enumerate()
            .filter(|(at, card)| card.source.file == path && own.contains_key(at))
            .filter_map(|(at, card)| {
                let hits = file.hits.iter().filter(|(line, _)| {
                    (*line >= card.source.line) && (*line <= card.source.end_line)
                });
                // Attribute an occurrence to the innermost current declaration,
                // preventing an enclosing type from borrowing all child clues.
                let mut direct = BTreeSet::new();
                for (line, matched) in hits {
                    if !cards.iter().any(|child| {
                        child.source.file == path
                            && child.source.line > card.source.line
                            && child.source.end_line <= card.source.end_line
                            && child.source.line <= *line
                            && *line <= child.source.end_line
                    }) {
                        direct.extend(matched.iter().copied());
                    }
                }
                (!direct.is_empty()).then_some((at, direct.len(), own[&at]))
            })
            .collect();
        choices.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(a.0.cmp(&b.0)));
        // Only explicit intent can replace the existing representative. Native
        // natural-language reranking remains separately measured before rollout.
        if !task.intent.trim().is_empty()
            && query.symbol.is_none()
            && !cards[*i].name.eq_ignore_ascii_case(query.text.trim())
            && let Some(&(winner, _, matched)) = choices.first()
            && matched > own.get(i).copied().unwrap_or(0)
            && matched >= 2
        {
            *i = winner;
            promotions += 1;
        }
        let choices: Vec<_> = choices
            .into_iter()
            .filter(|(at, _, _)| *at != *i)
            .take(2)
            .map(|(at, _, _)| cards[at].clone())
            .collect();
        if !choices.is_empty() {
            alternatives.insert(cards[*i].id.clone(), choices);
        }
    }
    ranking.dedup();
    let indexed: BTreeSet<_> = cards
        .iter()
        .filter(|card| files.contains_key(&card.source.file) && current(tree, &card.source, hashes))
        .map(|card| card.source.file.as_str())
        .collect();
    let mut live_only = Vec::new();
    for (path, file) in &files {
        if indexed.contains(path.as_str()) || file.hits.is_empty() {
            continue;
        }
        let line = file
            .hits
            .iter()
            .max_by_key(|(_, slots)| slots.len())
            .map_or(1, |(line, _)| *line);
        let source = Source {
            file: path.clone(),
            line,
            end_line: line,
            sha256: file.hash.clone(),
        };
        live_only.push(json!({"source":source,"kind":"live-line","indexed_symbol":null,"reason":"source-not-indexed-or-changed","excerpt":excerpt(file,line,line,7)}));
    }
    let live_only_count = live_only.len();
    live_only.truncate(query.limit.clamp(1, 16));
    omitted |= live_only_count > live_only.len();
    phases.push(json!({"operation":"current-source-search","files_read":files.len(),"bytes_read":bytes,"skipped_or_unavailable":skipped,
        "fallback":fallback,"repository_listing":repository_listing,"partial":omitted}));
    phases.push(json!({"operation":"cross-check","intent_promotions":promotions,"alternative_groups":alternatives.len(),"unindexed_matches":live_only_count}));
    let report = json!({"purpose":task.purpose,"intent":task.intent,"phases":phases,"live_matches":live_only,
        "partial":omitted,"source_scope":"indexed candidate files plus current changes; repository fallback only without indexed destinations",
        "stop_reason":if omitted {"partial-evidence-expand-or-use-original-search"} else {"available-evidence-returned"},
        "semantic_completeness":"unknown","local_model_calls":0,"remote_model_calls":0});
    Ok(Investigation {
        files,
        report,
        omitted,
        alternatives,
    })
}

fn excerpt(file: &File, first: u64, last: u64, limit: usize) -> Value {
    let rows: Vec<_> = file.text.lines().collect();
    let first = first.max(1).min(rows.len() as u64);
    let last = last.max(first).min(rows.len() as u64);
    let center = file
        .hits
        .iter()
        .filter(|(line, _)| *line >= first && *line <= last)
        .max_by_key(|(_, slots)| slots.len())
        .map_or(first, |(line, _)| *line);
    let start = if last - first < limit as u64 {
        first
    } else {
        center
            .saturating_sub((limit / 2) as u64)
            .max(first)
            .min(last.saturating_sub(limit as u64 - 1).max(first))
    };
    let end = last.min(start + limit as u64 - 1);
    let text = rows
        .iter()
        .enumerate()
        .filter(|(at, _)| (*at as u64 + 1 >= start) && ((*at as u64) < end))
        .map(|(at, line)| {
            format!(
                "{} | {}",
                at + 1,
                line.chars().take(500).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    json!({"line":start,"end_line":end,"text":text,"truncated":start>first || end<last
        || rows.iter().skip(start.saturating_sub(1) as usize).take(end.saturating_sub(start) as usize + 1).any(|line|line.chars().count()>500)})
}

impl Investigation {
    pub(super) fn adorn(
        &self,
        card: &Card,
        item: &mut Value,
        purpose: Purpose,
        primary_ids: &BTreeSet<&str>,
    ) {
        if let Some(file) = self
            .files
            .get(&card.source.file)
            .filter(|file| file.hash == card.source.sha256)
        {
            let limit = match purpose {
                Purpose::Locate => 7,
                Purpose::Understand | Purpose::Spec => 15,
                Purpose::Implement => 80,
                Purpose::Validate => 30,
            };
            item["source_excerpt"] = excerpt(file, card.source.line, card.source.end_line, limit);
        }
        if let Some(cards) = self.alternatives.get(&card.id) {
            let candidates:Vec<_> = cards.iter().filter(|other|!primary_ids.contains(other.id.as_str())).map(|other| {
                let mut value=json!({"id":other.id,"name":other.name,"signature":other.signature,"source":other.source,"status":"candidate; inspect evidence"});
                if purpose != Purpose::Locate && let Some(file)=self.files.get(&other.source.file).filter(|file|file.hash==other.source.sha256) {
                    value["source_excerpt"]=excerpt(file,other.source.line,other.source.end_line,7);
                }
                value
            }).collect();
            if !candidates.is_empty() {
                item["alternatives"] = json!(candidates);
            }
        }
    }

    pub(super) fn verify(&self, tree: &Path) -> bool {
        self.files.iter().all(|(path, file)| {
            source_bytes(tree, path).is_some_and(|bytes| hash(&bytes) == file.hash)
        })
    }
}
