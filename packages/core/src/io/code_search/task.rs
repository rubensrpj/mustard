//! Compose the existing investigation into a task-facing gateway response.
//! Source inventory precedes discovery; provider decisions never become facts.
use super::{Answer, Request, scope};
use crate::domain::knowledge::{
    Card, Source,
    investigation::{Matcher, Purpose, Task},
    selection::{self, Decisions, Outcome, SymbolSelector},
};
use crate::domain::normalize::Languages;
use crate::io::knowledge::{self, EvidenceScope, Query, investigation};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::Path;

pub(super) fn requested(request: &Request) -> bool {
    request.purpose != Purpose::Locate
        && !request.intent.trim().is_empty()
        && request.tool != "Read"
        && !(request.tool == "Grep" && request.input["output_mode"] == "count")
}

pub(super) fn investigate(
    root: &Path,
    tree: &Path,
    cwd: &Path,
    request: &Request,
    answer: &Answer,
    selector: Option<&dyn SymbolSelector>,
) -> Result<Value, String> {
    let scope = scope::inventory(tree, cwd, request, answer)?;
    let mut seeds: Vec<_> = answer.report["evidence"]["current_owner_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    let mut follow_up = Value::Null;
    if request.tool == "Grep" && answer.report["result"]["mode"] == "files_with_matches" {
        // Discovery gives paths, not matching declarations. Resolve the
        // pattern to line coordinates internally before asking the catalogue.
        let mut content = request.clone();
        content.input["output_mode"] = json!("content");
        content.input["-n"] = json!(true);
        content.input["head_limit"] = json!(0);
        content.input["offset"] = json!(0);
        let found = super::run(cwd, &content)?;
        let hits = super::occurrences(tree, cwd, &content, &found.report["result"], &found.stdout);
        let borrowed: Vec<_> = hits
            .iter()
            .map(|(file, line, text)| investigation::Occurrence {
                file,
                line: *line,
                text,
            })
            .collect();
        if let Ok(crossed) = investigation::cross_hits(root, tree, &borrowed) {
            for id in crossed["current_owner_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !seeds.iter().any(|seed| seed == id) {
                    seeds.push(id.into());
                }
            }
        }
        follow_up = json!({"operation":"file-discovery-to-native-content","occurrences_crossed":hits.len(),"source_bytes":found.stdout.len(),"native_original_preserved":true});
    }
    let opts = Query {
        text: if scope.clues.trim().is_empty() {
            &request.intent
        } else {
            &scope.clues
        },
        file: None,
        limit: 12,
        depth: 1,
        all: false,
        detail: true,
        symbol: None,
        direction: knowledge::Direction::Both,
        refresh: false,
    };
    let generation = knowledge::generation(root).map_err(|e| format!("{e:?}"))?;
    let (mut report, _) = knowledge::query_for_scope(
        root,
        tree,
        &opts,
        Task {
            intent: &request.intent,
            purpose: request.purpose,
        },
        &EvidenceScope {
            files: &scope.files,
            seeds: &seeds,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    if !knowledge::generation(root).is_ok_and(|now| now == generation) {
        return Err("task-scan-changed-during-discovery".into());
    }
    let languages = Languages::of_project(root);
    let registry = crate::domain::knowledge::resources::Registry::load()?;
    let mut matcher = Matcher::new(&request.intent, &languages)?;
    let mut cards = Vec::new();
    let mut source_text = BTreeMap::new();
    let mut source_hashes = BTreeMap::new();
    let mut alternative_ids = BTreeSet::new();
    // Include explicit alternatives in the decision, even when they lose the
    // lexical reading order. A winner cannot hide a competing declaration.
    for item in report["cards"].as_array().into_iter().flatten() {
        let card: Card = serde_json::from_value(item.clone()).map_err(|e| e.to_string())?;
        cards.push(card);
        for alternative in item["alternatives"].as_array().into_iter().flatten() {
            if let Some(id) = alternative["id"].as_str() {
                alternative_ids.insert(id.to_string());
            }
        }
    }
    cards.extend(knowledge::cards_for_ids(root, &alternative_ids).map_err(|e| format!("{e:?}"))?);
    let mut seen = BTreeSet::new();
    cards.retain(|card| seen.insert(card.id.clone()) && scope.files.contains(&card.source.file));
    let targets: BTreeSet<_> = cards
        .iter()
        .flat_map(|card| &card.outgoing)
        .filter_map(|edge| edge["target"].as_str())
        .filter(|id| !seen.contains(*id))
        .map(str::to_string)
        .collect();
    let target_ids = targets.iter().take(128).cloned().collect();
    let mut reference_matcher =
        Matcher::new(&format!("{} {}", scope.clues, request.intent), &languages)?;
    let references=knowledge::cards_for_ids(root,&target_ids).map_err(|e|format!("{e:?}"))?.into_iter()
        .filter(|card|scope.files.contains(&card.source.file) && knowledge::current(tree,&card.source,&mut source_hashes))
        .map(|card|json!({"id":card.id,"name":card.name,"source":card.source,"signature":card.signature,"initial_reference":!reference_matcher.matched(&card.name).is_empty(),"status":"static target candidate; source current, runtime behavior unverified"})).collect::<Vec<_>>();
    report["static_references"] = json!(references);
    report["static_references_partial"] = json!(targets.len() > 128);
    for card in &cards {
        if !source_text.contains_key(&card.source.file) {
            let text = investigation::safe_read(tree, &card.source.file, &registry)
                .ok_or("task-source-unavailable")?;
            source_text.insert(card.source.file.clone(), text);
        }
        if !knowledge::current(tree, &card.source, &mut source_hashes) {
            return Err("task-source-changed-before-selection".into());
        }
    }
    let mut plan = selection::responsibility(&cards, &request.intent, &languages);
    for group in &mut plan.groups {
        for card in &group.candidates {
            if let Some(text) = source_text.get(&card.source.file) {
                group.excerpts.insert(
                    card.id.clone(),
                    investigation::selection_excerpt(card, text, None),
                );
            }
        }
    }
    let mut recommendations = plan.recommendations;
    let mut usage = selection::native_usage();
    let mut outcomes = BTreeMap::new();
    let mut stages = Vec::new();
    let live_facts: Vec<_> = report["investigation"]["live_matches"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let file = item["source"]["file"].as_str()?;
            let line = item["source"]["line"].as_u64()?;
            let (_, text) = item["excerpt"]["text"].as_str()?.split_once(" | ")?;
            Some((file.to_string(), line, text.to_string()))
        })
        .collect();
    let pending = knowledge::observations::record(root, tree, &live_facts)
        .is_ok_and(|learning| learning["needs_scan"] == true);
    if !verify(root, tree, &generation, &cards, &report) {
        return Err("task-source-changed-before-selection".into());
    }
    if let Some(selector) =
        selector.filter(|_| request.choose && !pending && !plan.groups.is_empty())
    {
        let first = selector.select(&request.intent, &plan.groups);
        usage = first.usage.clone();
        stages.push(first.usage.clone());
        accept(&plan.groups, first, &mut recommendations, &mut outcomes);
        // Refine only an explicit lack of evidence, only with additional
        // current source, and at most once. Low confidence and transport
        // failures do not trigger a repeated paid guess.
        let mut refine = Vec::new();
        for group in &plan.groups {
            if outcomes.get(&group.key) != Some(&Outcome::InsufficientEvidence) {
                continue;
            }
            let mut next = group.clone();
            let mut changed = false;
            for card in &next.candidates {
                if next.excerpts.get(&card.id).is_some_and(|e| e.complete) {
                    continue;
                }
                if let Some(text) = source_text.get(&card.source.file) {
                    let lines: Vec<_> = text
                        .lines()
                        .skip(card.source.line.saturating_sub(1) as usize)
                        .take((card.source.end_line - card.source.line + 1) as usize)
                        .collect();
                    if lines.len() <= 256 && text.len() <= 2 * 1024 * 1024 {
                        let expanded =
                            lines
                                .iter()
                                .enumerate()
                                .fold(String::new(), |mut text, (at, line)| {
                                    let _ =
                                        writeln!(text, "{}:{line}", card.source.line + at as u64);
                                    text
                                });
                        if expanded.len() <= 16 * 1024 {
                            next.excerpts.insert(
                                card.id.clone(),
                                selection::Excerpt {
                                    source: card.source.clone(),
                                    text: expanded,
                                    complete: true,
                                },
                            );
                            changed = true;
                        }
                    }
                }
            }
            if changed {
                refine.push(next);
            }
        }
        if !refine.is_empty() && verify(root, tree, &generation, &cards, &report) {
            let second = selector.select(&request.intent, &refine);
            stages.push(second.usage.clone());
            accept(&refine, second, &mut recommendations, &mut outcomes);
            usage = combined_usage(&stages);
        }
    }
    if !verify(root, tree, &generation, &cards, &report) {
        return Ok(
            json!({"status":"discarded-source-changed","cards":[],"selection":usage,"selection_stages":stages,"remote_model_calls":usage["remote_model_calls"],"reason":"Source or scan changed during investigation; repeat the native search. Physical usage remains in the judgement ledger."}),
        );
    }
    let mut facts = live_facts;
    let mut covered = BTreeSet::new();
    let mut items = Vec::new();
    let mut item_slots = Vec::new();
    for card in &cards {
        let text = &source_text[&card.source.file];
        let mut item = crate::domain::knowledge::summary(card);
        let excerpt = investigation::current_excerpt(card, text, &mut matcher, request.purpose);
        let first = excerpt["line"].as_u64().unwrap_or(card.source.line);
        let last = excerpt["end_line"].as_u64().unwrap_or(first);
        let mut own_slots = BTreeSet::new();
        for (at, line) in text
            .lines()
            .enumerate()
            .filter(|(at, _)| *at as u64 + 1 >= first && (*at as u64) < last)
        {
            own_slots.extend(matcher.matched(line));
            facts.push((card.source.file.clone(), at as u64 + 1, line.to_string()));
        }
        item["source_excerpt"] = excerpt;
        item["recommended"] = json!(recommendations.contains(&card.id));
        item["native_search_owner"] = json!(seeds.contains(&card.id));
        item["read"] = json!({"tool":"Read","input":{"file_path":card.source.file,"offset":card.source.line,"limit":card.source.end_line-card.source.line+1}});
        item["body_comment"] = json!(card.body_comment);
        item["tests"] = json!(card.tests);
        item["history"] = json!({"command":"mustard-rt","args":["run","map","history","--file",card.source.file,"--name",card.name]});
        item["retrieval"] = report["cards"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|item| item["id"] == card.id)
            .map_or(json!("alternative"), |item| item["retrieval"].clone());
        items.push(item);
        item_slots.push(own_slots);
    }
    let primary = report["cards"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["id"].as_str())
        .map(str::to_string)
        .collect();
    let bodies = selection::task_bodies(
        &cards,
        &item_slots,
        &primary,
        &recommendations,
        &request.intent,
        &languages,
    );
    for (i, item) in items.iter_mut().enumerate() {
        item["initial_source_excerpt"] = json!(bodies.contains(&i));
        if bodies.contains(&i) {
            covered.extend(item_slots[i].iter().copied());
        }
    }
    for item in report["investigation"]["live_matches"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some(excerpt) = item["excerpt"]["text"].as_str()
            && let Some((_, text)) = excerpt.split_once(" | ")
        {
            covered.extend(matcher.matched(text));
        }
    }
    let uncovered: Vec<_> = matcher
        .slots
        .iter()
        .enumerate()
        .filter(|(at, _)| !covered.contains(at))
        .map(|(_, slot)| slot.clone())
        .collect();
    report["cards"] = json!(items);
    report["status"] = json!("current-task-evidence");
    report["scope"] = json!({"method":scope.method,"files":scope.files.len(),"native_query_rewritten":false,"outside_scope_reads":false});
    report["native_follow_up"] = follow_up;
    report["selection"] = usage;
    report["selection"]["outcomes"] = json!(outcomes);
    report["selection_stages"] = json!(stages);
    report["recommended_symbols"] = json!(recommendations);
    report["selection_basis"] = json!(plan.basis);
    report["remote_model_calls"] = report["selection"]["remote_model_calls"].clone();
    report["written_clues"] = json!({"covered_slots":covered.len(),"slots":matcher.slots.len(),"uncovered":uncovered,"meaning":"written clue coverage only; not semantic completeness or proof of absence"});
    report["native_fallback"] = json!({"request":Request{purpose:Purpose::Locate,choose:false,..request.clone()},"raw_native":"Repeat the same CLI request with --raw for exact native stdout/stderr/status."});
    report["learning"] = knowledge::observations::record(root, tree, &facts)
        .unwrap_or_else(|reason| json!({"status":"not-stored","reason":reason}));
    Ok(report)
}

fn accept(
    groups: &[selection::Ambiguity],
    decisions: Decisions,
    recommendations: &mut Vec<String>,
    outcomes: &mut BTreeMap<String, Outcome>,
) {
    for (key, id) in decisions.choices {
        if groups
            .iter()
            .any(|group| group.key == key && group.candidates.iter().any(|c| c.id == id))
            && !recommendations.contains(&id)
        {
            recommendations.push(id);
        }
    }
    outcomes.extend(
        decisions
            .outcomes
            .into_iter()
            .filter(|(key, _)| groups.iter().any(|group| &group.key == key)),
    );
}

fn verify(root: &Path, tree: &Path, generation: &str, cards: &[Card], report: &Value) -> bool {
    if !knowledge::generation(root).is_ok_and(|now| now == generation) {
        return false;
    }
    let mut hashes = BTreeMap::new();
    cards
        .iter()
        .all(|card| knowledge::current(tree, &card.source, &mut hashes))
        && report["resources"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|item| {
                serde_json::from_value::<Source>(item["source"].clone())
                    .is_ok_and(|source| knowledge::current(tree, &source, &mut hashes))
            })
        && report["static_references"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|item| {
                serde_json::from_value::<Source>(item["source"].clone())
                    .is_ok_and(|source| knowledge::current(tree, &source, &mut hashes))
            })
        && report["investigation"]["live_matches"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|item| {
                serde_json::from_value::<Source>(item["source"].clone())
                    .is_ok_and(|source| knowledge::current(tree, &source, &mut hashes))
            })
        && report["interpretations"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|item| item["sources"].as_array().into_iter().flatten())
            .all(|source| {
                serde_json::from_value::<Source>(source.clone())
                    .is_ok_and(|source| knowledge::current(tree, &source, &mut hashes))
            })
}

fn combined_usage(stages: &[Value]) -> Value {
    let sum = |field: &str| {
        stages.iter().try_fold(0_u64, |total, item| {
            item[field].as_u64().map(|n| total.saturating_add(n))
        })
    };
    json!({"status":"progressive-choice","remote_model_calls":sum("remote_model_calls"),"input_tokens":sum("input_tokens"),"known_input_tokens":stages.iter().filter_map(|item|item["known_input_tokens"].as_u64()).sum::<u64>(),"cost_micro_usd":sum("cost_micro_usd"),"usage_complete":stages.iter().all(|item|item["usage_complete"]==true)})
}
