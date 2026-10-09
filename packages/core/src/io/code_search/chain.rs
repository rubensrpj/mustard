//! Evidence-directed, deterministic follow-up. No inferred business behavior.
//! Follow current unique parser targets even without lexical query overlap.
use crate::domain::knowledge::{Card, Source, resources::Registry};
use crate::domain::knowledge::investigation::{Matcher, Purpose};
use crate::io::knowledge::{self, investigation};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(super) fn expand(root: &Path, tree: &Path, files: &BTreeSet<String>, seeds: &[Card],
    items: &mut Vec<Value>, matcher: &mut Matcher, purpose: Purpose, intent:&str) -> Result<Value, String> {
    let registry = Registry::load()?;
    let visible: BTreeSet<_> = items.iter().filter_map(|item| item["id"].as_str().map(str::to_string)).collect();
    let seed_ids: BTreeSet<_> = seeds.iter().take(4).map(|c| c.id.as_str()).collect();
    let targets: BTreeSet<_> = seeds.iter().filter(|c| seed_ids.contains(c.id.as_str())).flat_map(|c| &c.outgoing)
        .filter(|e| e["resolution"] == "unique-static-target")
        .filter_map(|e| e["target"].as_str().map(str::to_string)).collect();
    let candidates = knowledge::cards_for_ids(root, &targets.iter().take(128).cloned().collect()).map_err(|e| format!("{e:?}"))?;
    let mut hashes = BTreeMap::new();
    let mut steps = Vec::new();
    let mut appended = 0;
    let names=crate::domain::code_search::pattern_names(intent);
    let covered:BTreeSet<_>=items.iter().filter(|item|item["initial_source_excerpt"]==true)
        .flat_map(|item|matcher.matched(item["source_excerpt"]["text"].as_str().unwrap_or_default())).collect();
    for card in candidates.iter().filter(|c| files.contains(&c.source.file)) {
        if steps.len()>=8 {break;}
        if !knowledge::current(tree, &card.source, &mut hashes) { continue; }
        let Some(text) = investigation::safe_read(tree, &card.source.file, &registry) else { continue; };
        let seed = seeds.iter().find(|seed| seed.outgoing.iter().any(|e| e["target"] == card.id && e["resolution"] == "unique-static-target"));
        let Some(seed) = seed else { continue; };
        let edge = seed.outgoing.iter().find(|e| e["target"] == card.id);
        let call_line = edge.and_then(|e| e["call_line"].as_u64()).unwrap_or(seed.source.line);
        let call = investigation::safe_read(tree, &seed.source.file, &registry)
            .filter(|text| digest(text) == seed.source.sha256)
            .and_then(|text| text.lines().nth(call_line.saturating_sub(1) as usize).map(str::to_string));
        let Some(call) = call else { continue; };
        steps.push(json!({"operation":"unique-static-dependency","from":seed.id,"to":card.id,
            "call_source":Source{file:seed.source.file.clone(),line:call_line,end_line:call_line,sha256:seed.source.sha256.clone()},
            "call_text":call,"target_source":card.source,"signature":card.signature,"syntax":card.syntax,
            "meaning":"single written call line and declared signature/types; arguments may continue; not proven values or persistence effects"}));
        let excerpt = investigation::current_excerpt(card, &text, matcher, purpose);
        let written=format!("{} {} {}",card.signature,card.documentation,excerpt["text"].as_str().unwrap_or_default());
        let introduces_clue=matcher.matched(&written).iter().any(|slot|!covered.contains(slot));
        let expand=appended<2 && (names.iter().any(|name|name==&card.name) || introduces_clue);
        if visible.contains(&card.id) {
            if let Some(item)=items.iter_mut().find(|item|item["id"]==card.id) {
                item["initial_reference"]=json!(true);
                if expand && item["initial_source_excerpt"]!=true {
                    item["initial_source_excerpt"]=json!(true);item["source_excerpt"]=excerpt;
                    item["follow_up_reason"]=json!("dependency-adds-written-question-evidence");appended+=1;
                }
            }
            continue;
        }
        let mut item = crate::domain::knowledge::summary(card);
        item["source_excerpt"] = excerpt;
        item["initial_source_excerpt"] = json!(expand);
        item["initial_reference"] = json!(true);
        item["recommended"] = json!(false);
        item["retrieval"] = json!("unique-static-dependency");
        item["read"] = json!({"tool":"Read","input":{"file_path":card.source.file,"offset":card.source.line,"limit":card.source.end_line-card.source.line+1}});
        item["tests"] = json!(card.tests);
        items.push(item);
        appended += usize::from(expand);
    }
    let mut tests = Vec::new();
    let mut checked = BTreeSet::new();
    for seed in seeds.iter().take(4) {
        for file in seed.tests.iter().filter(|file| files.contains(*file)) {
            if checked.len() >= 8 { break; }
            if !checked.insert((file.clone(), seed.name.clone())) { continue; }
            let Some(text) = investigation::safe_read(tree, file, &registry) else { continue; };
            for (at, line) in text.lines().enumerate().filter(|(_,line)| name_mentioned(line,&seed.name)).take(2) {
                tests.push(json!({"symbol":seed.id,"source":Source{file:file.clone(),line:at as u64+1,end_line:at as u64+1,sha256:digest(&text)},
                    "text":line,"meaning":"current textual mention in associated test; execution and coverage unverified"}));
            }
        }
    }
    let partial=targets.len()>steps.len();
    Ok(json!({"steps":steps,"test_mentions":tests,"appended_bodies":appended,
        "partial":partial,"local_model_calls":0,"remote_model_calls":0,
        "meaning":"bounded source-backed follow-ups within original inventory; no semantic completeness claim"}))
}
fn digest(text: &str) -> String {
    let mut h = crate::io::sha256::Sha256::new(); h.update(text.as_bytes()); h.hex_digest()
}
fn name_mentioned(line: &str, name: &str) -> bool {
    let part = |c:char| c.is_alphanumeric() || matches!(c,'_'|'$');
    !name.is_empty() && line.match_indices(name).any(|(at,_)|
        line[..at].chars().next_back().is_none_or(|c|!part(c))
        && line[at+name.len()..].chars().next().is_none_or(|c|!part(c)))
}
