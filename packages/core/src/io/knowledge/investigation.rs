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
    cross(root, tree, hits, "", None, false, Purpose::Locate)
}

/// Classify responsibility only among current owners of actual source hits.
/// The native result is held by the caller and cannot be pruned by a selector.
pub fn cross_hits_for(
    root: &Path,
    tree: &Path,
    hits: &[Occurrence<'_>],
    intent: &str,
    purpose: Purpose,
    selector: Option<&dyn knowledge::selection::SymbolSelector>,
) -> Result<Value, MapRefusal> {
    cross(root, tree, hits, intent, selector, true, purpose)
}

fn cross(
    root: &Path,
    tree: &Path,
    hits: &[Occurrence<'_>],
    intent: &str,
    selector: Option<&dyn knowledge::selection::SymbolSelector>,
    rich: bool,
    purpose: Purpose,
) -> Result<Value, MapRefusal> {
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
            let Some(text) = safe_read(tree, hit.file, &registry) else {
                if rich {
                    unmatched += 1;
                    processed += 1;
                    continue;
                }
                return Err(invalid("knowledge-live-source-unavailable"));
            };
            bytes += text.len();
            files.insert(hit.file.to_string(), (hash(text.as_bytes()), text));
        }
        let Some((digest, text)) = files.get(hit.file) else {
            continue;
        };
        if hit.line == 0 || text.lines().nth(hit.line.saturating_sub(1) as usize) != Some(hit.text)
        {
            if rich {
                unmatched += 1;
                processed += 1;
                continue;
            }
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
    let mut cards = catalog::hydrate(db.conn(), &ids).map_err(super::unreadable)?;
    // Do not pay for a choice over a scan/source snapshot already invalidated.
    if before != super::generation(root)?
        || !files.iter().all(|(path, (digest, _))| {
            source_bytes(tree, path).is_some_and(|bytes| hash(&bytes) == *digest)
        })
    {
        return Err(invalid("knowledge-live-evidence-changed"));
    }
    let total = cards.len();
    let mut plan = knowledge::selection::responsibility(&cards, intent, &Languages::of_project(root));
    let mut recommendation = plan.recommendations;
    // IO supplies checked source; the domain planner and provider remain
    // independent of filesystems, languages, frameworks and search commands.
    if selector.is_some() {
        for group in &mut plan.groups {
            for card in &group.candidates {
                if let Some((_, text)) = files.get(&card.source.file) {
                    group.excerpts.insert(card.id.clone(), selection_excerpt(card, text, matched.get(&card.id)));
                }
            }
        }
    }
    let mut selection = json!({"status":"native","remote_model_calls":0});
    let mut remaining_ambiguities = plan.groups.len();
    if !plan.groups.is_empty()
        && let Some(selector) = selector
    {
        let decisions = selector.select(intent, &plan.groups);
        // Reject an adapter choice that was not supplied as current evidence.
        let accepted: Vec<_> = decisions
            .choices
            .into_iter()
            .filter(|(key, id)| {
                plan.groups.iter().any(|group| {
                    group.key == *key && group.candidates.iter().any(|c| c.id == *id)
                })
            })
            .map(|(_, id)| id)
            .collect();
        let no_match = decisions.outcomes.iter().filter(|(key, outcome)| {
            **outcome == knowledge::selection::Outcome::NoMatch && plan.groups.iter().any(|group| &group.key == *key)
        }).count();
        remaining_ambiguities = remaining_ambiguities.saturating_sub(accepted.len() + no_match);
        recommendation.extend(accepted);
        selection = decisions.usage;
        selection["outcomes"] = json!(decisions.outcomes);
    }
    if rich {
        let order: BTreeMap<_, _> = plan.order.iter().enumerate().map(|(rank, &i)| (cards[i].id.clone(), rank)).collect();
        cards.sort_by_key(|card| (!recommendation.contains(&card.id), order.get(&card.id).copied().unwrap_or(usize::MAX)));
    }
    let evidence:Vec<_>=cards.iter().take(4).map(|card| {
        if rich {
            let mut item=if purpose==Purpose::Locate {
                json!({"id":card.id,"name":card.name,"kind":card.kind,"source":card.source,
                    "signature":card.signature.chars().take(160).collect::<String>(),"documentation":card.documentation.chars().take(160).collect::<String>()})
            }else{knowledge::summary(card)};
            item["matched_lines"]=json!(matched.get(&card.id));
            item["status"]=json!("current-static-owner");
            if purpose!=Purpose::Locate {
                if !card.body_comment.is_empty() {item["body_comment"]=json!(card.body_comment.chars().take(240).collect::<String>());}
                if !card.tests.is_empty() {item["tests"]=json!(card.tests.iter().take(3).collect::<Vec<_>>());}
            }
            item["read"]=json!({"tool":"Read","input":{"file_path":tree.join(&card.source.file),"offset":card.source.line,
                "limit":card.source.end_line.saturating_sub(card.source.line)+1}});
            item["expand"]=json!({"command":"mustard-rt","args":["run","knowledge","--symbol",card.id,"--purpose","implement","--root",tree]});
            item["history"]=json!({"command":"mustard-rt","args":["run","map","history","--file",card.source.file,"--name",card.name]});
            return item;
        }
        json!({"id":card.id,"name":card.name,"source":card.source,
        "signature":card.signature,"matched_lines":matched.get(&card.id),"status":"current-static-owner",
        "expand":{"command":"mustard-rt","args":["run","knowledge","--symbol",card.id,"--purpose","implement"]}})
    }).collect();
    if before != super::generation(root)?
        || !files.iter().all(|(path, (digest, _))| {
            source_bytes(tree, path).is_some_and(|bytes| hash(&bytes) == *digest)
        })
    {
        return Err(invalid("knowledge-live-evidence-changed"));
    }
    let mut report = json!({"symbols":evidence,"current_owner_ids":cards.iter().map(|card|&card.id).collect::<Vec<_>>(),"omitted_symbols":total.saturating_sub(4),"unmapped_occurrences":unmatched,
        "omitted_occurrences":hits.len().saturating_sub(processed),"semantic_completeness":"unknown","local_model_calls":0,"remote_model_calls":0});
    if rich {
        let mut outlines = Vec::new();
        for (path, (digest, _)) in files.iter().take(4) {
            if cards.iter().any(|card| &card.source.file == path) {
                continue;
            }
            let mut statement=db.conn().prepare("SELECT id,name,line,end_line FROM knowledge_symbols WHERE path=?1 AND sha256=?2 ORDER BY line LIMIT 8").map_err(|e|super::unreadable(e.into()))?;
            let rows=statement.query_map(rusqlite::params![path,digest],|row|Ok(json!({"id":row.get::<_,String>(0)?,"name":row.get::<_,String>(1)?,"line":row.get::<_,i64>(2)?,"end_line":row.get::<_,i64>(3)?}))).map_err(|e|super::unreadable(e.into()))?;
            let declarations = rows
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| super::unreadable(e.into()))?;
            if !declarations.is_empty() {
                outlines.push(json!({"file":path,"sha256":digest,"declarations":declarations,"status":"current-static-outline","limit":8}));
            }
        }
        report["files"] = json!(outlines);
        report["recommended_symbols"] = json!(recommendation);
        report["intent_requested"] = json!(intent.trim().is_empty() && total > 1);
        report["remaining_ambiguities"] = json!(remaining_ambiguities);
        report["selection"] = selection;
        report["selection_basis"] = json!(plan.basis);
        report["remote_model_calls"] = report["selection"]["remote_model_calls"].clone();
        report["relations_status"] =
            json!("static scan candidates; expand and verify target source before editing");
    }
    if before != super::generation(root)? {
        return Err(invalid("knowledge-live-evidence-changed"));
    }
    Ok(report)
}

/// Bounded, line-addressed source evidence. Small declarations are complete;
/// larger ones retain their boundaries and actual hit neighborhoods. A missing
/// middle is explicitly incomplete, never evidence that a behavior is absent.
pub(crate) fn selection_excerpt(card: &Card, text: &str, hits: Option<&Vec<u64>>) -> knowledge::selection::Excerpt {
    let source = &card.source;
    let lines: Vec<_> = text.lines().collect();
    let mut wanted = BTreeSet::new();
    for line in source.line..=source.end_line.min(source.line.saturating_add(31)) { wanted.insert(line); }
    for line in source.end_line.saturating_sub(15).max(source.line)..=source.end_line { wanted.insert(line); }
    for &hit in hits.into_iter().flatten() {
        for line in hit.saturating_sub(2).max(source.line)..=hit.saturating_add(2).min(source.end_line) { wanted.insert(line); }
    }
    let mut content = String::new();
    let mut included = 0_u64;
    for line in wanted.into_iter().take(64) {
        let Some(value) = usize::try_from(line.saturating_sub(1)).ok().and_then(|i| lines.get(i)) else { continue };
        let next = format!("{line}:{value}\n");
        if content.len() + next.len() > 4096 { continue; }
        content.push_str(&next); included += 1;
    }
    knowledge::selection::Excerpt {
        source: source.clone(), text: content,
        complete: included == source.end_line.saturating_sub(source.line) + 1,
    }
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

pub(crate) fn safe_read(
    tree: &Path,
    path: &str,
    registry: &knowledge::resources::Registry,
) -> Option<String> {
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

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    root: &Path,
    tree: &Path,
    query: &Query<'_>,
    task: Task<'_>,
    cards: &mut Vec<Card>,
    ranking: &mut Vec<usize>,
    hashes: &mut BTreeMap<String, Option<(String, u64)>>,
    scope: Option<&super::EvidenceScope<'_>>,
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
        let pool = if let Some(scope)=scope {catalog::scoped_candidates(root,&intent,scope)?} else {catalog::candidates(root, &intent, &[], None, &[], false)?};
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
    if let Some(scope)=scope {
        let weights=catalog::intent_weights(root,&phrase,&languages)?;
        let mut order=knowledge::selection::task_order(cards,&phrase,&languages,&weights,scope.seeds);
        for &i in ranking.iter() {if !order.contains(&i) {order.push(i);}}
        *ranking=order;
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
    if let Some(scope)=scope {
        for path in catalog::unindexed_files(root,scope)? {
            if !paths.contains(&path) {paths.push(path);}
        }
    }
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
            .filter(|path| !path.is_empty() && scoped(path, query.file) && scope.is_none_or(|scope|scope.files.contains(*path)))
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
                .filter(|path| !path.is_empty() && scoped(path, query.file) && scope.is_none_or(|scope|scope.files.contains(*path)))
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
        if scope.is_none() && !task.intent.trim().is_empty()
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
    let mut missing=Vec::new();
    if start>first {missing.push((first,start-1));}
    for line in start..=end {
        if rows.get(line.saturating_sub(1) as usize).is_some_and(|text|text.chars().count()>500) {
            missing.push((line,line));
        }
    }
    if end<last {missing.push((end+1,last));}
    let mut ranges:Vec<(u64,u64)>=Vec::new();
    for (line,end_line) in missing {
        if let Some(previous)=ranges.last_mut().filter(|previous|previous.1.saturating_add(1)>=line) {
            previous.1=previous.1.max(end_line);
        } else {ranges.push((line,end_line));}
    }
    json!({"line":start,"end_line":end,"text":text,"truncated":!ranges.is_empty(),
        "missing_ranges":ranges.into_iter().map(|(line,end_line)|json!({"line":line,"end_line":end_line})).collect::<Vec<_>>()})
}

pub(crate) fn current_excerpt(card: &Card, text: &str, matcher: &mut Matcher, purpose: Purpose) -> Value {
    let hits=text.lines().enumerate().filter_map(|(at,line)| {
        let slots=matcher.matched(line);
        (!slots.is_empty()).then_some((at as u64+1,slots))
    }).collect();
    let full=card.source.end_line-card.source.line<64 && text.lines().skip(card.source.line.saturating_sub(1) as usize).take((card.source.end_line-card.source.line+1) as usize).map(str::len).sum::<usize>()<=4096;
    excerpt(&File{text:text.into(),hash:card.source.sha256.clone(),hits},card.source.line,card.source.end_line,if full {64}else{purpose.excerpt_lines()})
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
            let limit = purpose.excerpt_lines();
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
