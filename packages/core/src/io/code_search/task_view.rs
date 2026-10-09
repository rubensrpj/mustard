//! Task evidence projection. Full native results remain a separate, explicit
//! representation; metadata and source snippets are not native occurrences.
use super::{Answer, Request, presentation::Presentation};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

fn quote(value:&str)->String {format!("'{}'",value.replace('\'',"'\\''"))}

pub(super) fn agent(answer: &Answer, request: &Request) -> Option<Presentation> {
    let context = &answer.report["task_context"];
    if context["status"] != "current-task-evidence" {
        return None;
    }
    let cards = context["cards"].as_array()?;
    let resources = context["resources"].as_array();
    let live = context["investigation"]["live_matches"].as_array();
    if context["written_clues"]["covered_slots"].as_u64() == Some(0)
        && cards
            .iter()
            .all(|card| card["initial_source_excerpt"] != true)
        && resources.is_none_or(Vec::is_empty)
    {
        return None;
    }
    if cards.is_empty()
        && resources.is_none_or(Vec::is_empty)
        && (live.is_none_or(Vec::is_empty)
            || context["written_clues"]["covered_slots"]
                .as_u64()
                .unwrap_or(0)
                == 0)
    {
        return None;
    }
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# task evidence ({:?}); current source, partial investigation",
        request.purpose
    );
    text.push_str("# Complete original result: repeat this request with purpose=locate; native CLI also supports --raw.\n");
    text.push_str("# Reuse complete bodies below; read only missing ranges when further source is required.\n");
    if request.tool == "Grep" {
        let _ = writeln!(
            text,
            "# Original page: offset {}, more {}. Complementary evidence below is a separate task view.",
            request.input["offset"].as_u64().unwrap_or(0),
            answer.report["result"]["truncated"]
                .as_bool()
                .unwrap_or(false)
        );
    }
    if let Some(files) = answer.report["query_quality"]["returned_files"].as_u64() {
        let _ = writeln!(
            text,
            "# Original discovery: {files} files. {}",
            answer.report["query_quality"]["next_step"]
                .as_str()
                .unwrap_or_default()
        );
    }
    if let Some(outcomes) = context["selection"]["outcomes"].as_object() {
        for outcome in outcomes.values().filter_map(Value::as_str) {
            let _ = writeln!(
                text,
                "# selection: {outcome}; supplied candidates only, verify source"
            );
        }
    }
    let mut previous = "";
    let mut test_files = BTreeSet::new();
    let mut ranges = BTreeMap::<&str, Vec<String>>::new();
    let mut deferred = BTreeMap::<&str, usize>::new();
    let mut visible_ids = BTreeSet::new();
    for card in cards {
        let file = card["source"]["file"].as_str()?;
        let start = card["source"]["line"].as_u64()?;
        let end = card["source"]["end_line"].as_u64()?;
        if card["initial_source_excerpt"] != true {
            if card["initial_reference"] == true {
                ranges.entry(file).or_default().push(format!("{} {start}-{end}",card["name"].as_str().unwrap_or_default()));
                visible_ids.insert(card["id"].as_str().unwrap_or_default());
            } else {*deferred.entry(file).or_default()+=1;}
            continue;
        }
        visible_ids.insert(card["id"].as_str().unwrap_or_default());
        if previous != file {
            let _ = writeln!(text, "@ {file}");
            previous = file;
        }
        let _ = writeln!(
            text,
            "# {} {start}-{end} [{}{}]",
            card["name"].as_str().unwrap_or_default(),
            card["retrieval"].as_str().unwrap_or("static evidence"),
            if card["recommended"] == true && context["selection_basis"]=="exact-symbol-identity" {
                "; exact name"
            } else if card["recommended"] == true {
                "; recommended"
            } else {
                ""
            }
        );
        let source = card["source_excerpt"]["text"].as_str().unwrap_or_default();
        for field in ["signature", "documentation", "body_comment"] {
            if field=="body_comment" && card["source_excerpt"]["truncated"]==false {continue;}
            if let Some(value) = card[field].as_str().filter(|v| {
                !v.is_empty() && !v.split_whitespace().all(|word| source.contains(word))
            }) {
                let _ = writeln!(text, "# {field}: {}", value.replace(['\r', '\n'], " "));
            }
        }
        for field in ["contracts", "routes", "annotations"] {
            if let Some(values) = card[field].as_array().filter(|values| !values.is_empty()) {
                let _ = writeln!(text, "# {field}: {}", Value::Array(values.clone()));
            }
        }
        let tests: Vec<_> = card["tests"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|file| test_files.insert(*file))
            .collect();
        if !tests.is_empty() {
            let _ = writeln!(
                text,
                "# test candidates (coverage unverified): {}",
                tests.iter().take(2).copied().collect::<Vec<_>>().join(", ")
            );
            if tests.len()>2 {
                let _=writeln!(text,"# {} more test candidates: mustard-rt run map tests --file {}",tests.len()-2,quote(file));
            }
        }
        if !source.is_empty() {
            text.push_str(source);
            text.push('\n');
        }
        if card["source_excerpt"]["truncated"] == true {
            text.push_str("# Incomplete excerpt. Missing source ranges in this file:\n");
            if let Some(reads)=card["missing_source_reads"].as_array().filter(|reads|!reads.is_empty()) {
                for read in reads {
                    let _=writeln!(text,"# Read offset {}, limit {}",read["input"]["offset"],read["input"]["limit"]);
                }
            } else {
                let _=writeln!(text,"# Read offset {start}, limit {}",end-start+1);
            }
        }
        if card["parse_complete"] != true {
            text.push_str("# Parse coverage partial or unknown.\n");
        }
    }
    for (file, candidates) in ranges {
        let _ = writeln!(
            text,
            "@ {file}\n# References (expand source/responsibility): {}",
            candidates.join("; ")
        );
    }
    for (file,count) in deferred {
        let _=writeln!(text,"# {count} additional candidates in {file}: mustard-rt run map summary --file {}",quote(file));
    }
    for edge in context["navigation"]["paths"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let _ = writeln!(
            text,
            "# static relation: {} -> {} ({}, runtime order unknown)",
            edge["from"].as_str().unwrap_or_default(),
            edge["to"].as_str().unwrap_or_default(),
            edge["via"].as_str().unwrap_or_default()
        );
    }
    for resource in resources.into_iter().flatten() {
        let _ = writeln!(
            text,
            "@ {}:{}-{} (verbatim resource; behavior unverified)\n{}",
            resource["source"]["file"].as_str().unwrap_or_default(),
            resource["source"]["line"],
            resource["source"]["end_line"],
            resource["text"].as_str().unwrap_or_default()
        );
    }
    let mut references = BTreeMap::<&str, Vec<String>>::new();
    for reference in context["static_references"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|reference| reference["initial_reference"] == true && !visible_ids.contains(reference["id"].as_str().unwrap_or_default()))
    {
        references
            .entry(reference["source"]["file"].as_str()?)
            .or_default()
            .push(format!(
                "{} {}-{}: {}",
                reference["name"].as_str().unwrap_or_default(),
                reference["source"]["line"],
                reference["source"]["end_line"],
                reference["signature"].as_str().unwrap_or_default().replace(['\r','\n']," ")
            ));
    }
    for (file, targets) in references {
        let _ = writeln!(
            text,
            "# Static targets in {file}: {}. Expand to verify behavior.",
            targets.join("; ")
        );
    }
    if context["static_references_partial"] == true {
        text.push_str("# Additional static target references omitted; expand an exact symbol.\n");
    }
    for item in live.into_iter().flatten() {
        let _ = writeln!(
            text,
            "@ {} (current source without an indexed symbol)\n{}",
            item["source"]["file"].as_str().unwrap_or_default(),
            item["excerpt"]["text"].as_str().unwrap_or_default()
        );
    }
    for note in context["interpretations"].as_array().into_iter().flatten() {
        let _ = writeln!(
            text,
            "# Interpretation [{}; author assertion]: {}",
            note["status"].as_str().unwrap_or_default(),
            note["text"].as_str().unwrap_or_default()
        );
    }
    for gap in context["gaps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if gap=="Static relations do not prove runtime order, authorization, business rules or test coverage." {continue;}
        if let Some(count)=context["navigation"]["ambiguous_relations"].as_u64()
            && gap.starts_with(&format!("{count} ambiguous static relations")) {
            let _=writeln!(text,"# Gap: {count} ambiguous static links; inspect an exact symbol to verify targets.");
        } else {
            let _ = writeln!(text, "# Gap: {gap}");
        }
    }
    text.push_str("# Partial evidence; static links/tests are candidates. Expand before inferring absence.\n");
    Some(Presentation {
        stdout: text.into_bytes(),
        representation: "current-task-evidence",
        owner_ranges: cards.len(),
    })
}
