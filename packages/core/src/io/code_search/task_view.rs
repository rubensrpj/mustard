//! Task evidence projection. Full native results remain a separate, explicit
//! representation; metadata and source snippets are not native occurrences.
use super::{Answer, Request, presentation::Presentation};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

fn quote(value:&str)->String {format!("'{}'",value.replace('\'',"'\\''"))}

pub(super) fn reuse_notice(card:&Value)->Option<String> {
    Some(format!("@ {}\n# Reuse {} {}-{}: complete body already delivered here; current hash. Scoped Read repeats.\n",
        card["source"]["file"].as_str()?,card["name"].as_str()?,card["source"]["line"].as_u64()?,card["source"]["end_line"].as_u64()?))
}

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
    text.push_str("# Reuse delivered source; read only missing ranges when further source is required.\n");
    if cards.iter().any(|card|card["test_only"]==true) {
        text.push_str("# Test-only declarations are marked; execution/coverage unverified.\n");
    }
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
    // The choice the Jev did not make in time stays with the agent, among the
    // candidates below.
    if let Some(left) = context["selection"]["choice_left"].as_str() {
        let _ = writeln!(text, "# {left}");
    }
    // An implementation search for a pattern: its hit lines come first, every
    // one of them, and a file the pattern did not hit is left out.
    let pattern: BTreeSet<&str> = context["pattern_files"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let shown = |file: &str| pattern.is_empty() || pattern.contains(file);
    let hit_card = |card: &Value| !pattern.is_empty() && card["shows_pattern_hit"] == true;
    let mut ordered: Vec<&Value> = cards.iter().filter(|card| hit_card(card)).collect();
    let files: Vec<&str> = cards.iter().filter(|card| hit_card(card)).filter_map(|card| card["source"]["file"].as_str()).collect();
    ordered.sort_by_key(|card| files.iter().position(|file| card["source"]["file"] == *file));
    let hit_cards = ordered.len();
    ordered.extend(cards.iter().filter(|card| !hit_card(card)));
    let mut lead = None;
    let mut previous = "";
    let mut test_files = BTreeSet::new();
    let mut ranges = BTreeMap::<&str, Vec<String>>::new();
    let mut test_ranges = BTreeMap::<&str, Vec<String>>::new();
    let mut deferred = BTreeMap::<&str, usize>::new();
    let mut visible_ids = BTreeSet::new();
    let mut source_view = super::source_view::View::default();
    for (at, card) in ordered.into_iter().enumerate() {
        if at == hit_cards && !pattern.is_empty() {
            pattern_lines(&mut text, context, live);
            lead = Some(std::mem::take(&mut text));
            previous = "";
        }
        let file = card["source"]["file"].as_str()?;
        if !shown(file) {continue;}
        let start = card["source"]["line"].as_u64()?;
        let end = card["source"]["end_line"].as_u64()?;
        if card["reused_delivery"] == true {
            text.push_str(&reuse_notice(card)?);
            visible_ids.insert(card["id"].as_str().unwrap_or_default());
            continue;
        }
        if card["initial_source_excerpt"] != true {
            if card["initial_reference"] == true {
                ranges.entry(file).or_default().push(format!("{} {start}-{end}",card["name"].as_str().unwrap_or_default()));
                if card["test_only"]==true {test_ranges.entry(file).or_default().push(format!("{start}-{end}"));}
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
            "# {} {start}-{end} [{}{}{}]",
            card["name"].as_str().unwrap_or_default(),
            card["retrieval"].as_str().unwrap_or("static evidence"),
            if card["recommended"] == true && context["selection_basis"]=="exact-symbol-identity" {
                "; exact name"
            } else if card["recommended"] == true {
                "; recommended"
            } else {
                ""
            },
            if card["test_only"]==true {"; test-only"}else{""}
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
            if let Some(ranges)=card["reused_source_ranges"].as_str() {
                let _=writeln!(text,"# Source lines {ranges} already delivered in this context; current hash.");
            }
            let delivered=card["delivery_source_excerpt"].as_str().unwrap_or(source);
            text.push_str(&source_view.render(file,card["source"]["sha256"].as_str().unwrap_or_default(),delivered));
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
    if lead.is_none() && !pattern.is_empty() {
        pattern_lines(&mut text, context, live);
        lead = Some(std::mem::take(&mut text));
    }
    // Static dependencies need a destination, not every full signature before
    // the agent has chosen to investigate it. Keep names and exact ranges in
    // the same navigable representation as the other expandable references.
    for reference in context["static_references"].as_array().into_iter().flatten()
        .filter(|reference| reference["initial_reference"] == true)
    {
        let id=reference["id"].as_str().unwrap_or_default();
        if !shown(reference["source"]["file"].as_str()?) || !visible_ids.insert(id) {continue;}
        ranges.entry(reference["source"]["file"].as_str()?).or_default().push(format!("{} {}-{}",
            reference["name"].as_str().unwrap_or_default(),reference["source"]["line"],reference["source"]["end_line"]));
    }
    for (file, candidates) in ranges {
        let _ = writeln!(
            text,
            "@ {file}\n# References (expand source/responsibility): {}",
            candidates.join("; ")
        );
        if let Some(tests)=test_ranges.get(file) {let _=writeln!(text,"# Test-only ranges: {}",tests.join("; "));}
    }
    graph(&mut text,context,&shown);
    for test in context["chain"]["test_mentions"].as_array().into_iter().flatten()
        .filter(|test|shown(test["source"]["file"].as_str().unwrap_or_default())) {
        let _=writeln!(text,"@ {}\n{} | {}\n# Associated test mention; coverage/execution unverified.",
            test["source"]["file"].as_str().unwrap_or_default(),test["source"]["line"],test["text"].as_str().unwrap_or_default());
    }
    for (at,question) in context["question_coverage"].as_array().into_iter().flatten().enumerate() {
        let _=writeln!(text,"# Question {}: {} — {} source references; behavior unverified",at+1,
            question["question"].as_str().unwrap_or_default(),question["references"].as_array().map_or(0,Vec::len));
    }
    if !deferred.is_empty() {
        text.push_str("# Deferred additional candidates: mustard-rt run map summary --file <file>\n");
        for (file,count) in deferred {
            let _=writeln!(text,"# {file}: {count}");
        }
    }
    for edge in context["navigation"]["paths"]
        .as_array()
        .into_iter()
        .flatten()
    {
        // A unique direct dependency already includes its call and target
        // ranges in Native follow-up. Keep other directions/resolutions and
        // longer paths; those are different evidence, not repeated wording.
        if edge["via"]=="outgoing" && edge["distance"]==1 && edge["resolution"]=="unique-static-target"
            && context["chain"]["steps"].as_array().into_iter().flatten().any(|step|
                step["operation"]=="unique-static-dependency" && step["from"]==edge["from"] && step["to"]==edge["to"]) {continue;}
        if !["from","to"].iter().all(|end|shown(declared_file(edge[*end].as_str().unwrap_or_default()))) {continue;}
        let _ = writeln!(
            text,
            "# static relation: {} -> {} ({}, runtime order unknown)",
            edge["from"].as_str().unwrap_or_default(),
            edge["to"].as_str().unwrap_or_default(),
            edge["via"].as_str().unwrap_or_default()
        );
    }
    for resource in resources.into_iter().flatten().filter(|resource|shown(resource["source"]["file"].as_str().unwrap_or_default())) {
        let _ = writeln!(
            text,
            "@ {}:{}-{} (verbatim resource; behavior unverified)\n{}",
            resource["source"]["file"].as_str().unwrap_or_default(),
            resource["source"]["line"],
            resource["source"]["end_line"],
            resource["text"].as_str().unwrap_or_default()
        );
    }
    if context["static_references_partial"] == true {
        text.push_str("# Additional static target references omitted; expand an exact symbol.\n");
    }
    if pattern.is_empty() {pattern_lines(&mut text, context, live);}
    let left_out = left_out(context, &pattern);
    if left_out > 0 {
        let _ = writeln!(text, "# Files without pattern hits left out: {left_out}; expand an exact symbol to follow its links.");
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
    // Hits keep their own section: merging a file's later references into it
    // would put them before the next file's hits.
    let text=match lead {
        Some(lead)=>coalesce_files(lead,context)+&coalesce_files(text,context),
        None=>coalesce_files(text,context),
    };
    Some(Presentation {
        stdout: text.into_bytes(),
        representation: "current-task-evidence",
        owner_ranges: cards.len(),
    })
}

/// Pattern hits no excerpt shows, and hit lines of files without an indexed
/// symbol, under their file.
fn pattern_lines(text: &mut String, context: &Value, live: Option<&Vec<Value>>) {
    let mut previous = "";
    let mut listed = BTreeSet::new();
    for item in context["native_lines"]["lines"].as_array().into_iter().flatten() {
        let file = item["source"]["file"].as_str().unwrap_or_default();
        if previous != file {
            let _ = writeln!(text, "@ {file}\n# Pattern hits not shown above:");
            previous = file;
        }
        listed.insert((file, item["source"]["line"].as_u64()));
        let _ = writeln!(text, "{} | {}", item["source"]["line"], item["text"].as_str().unwrap_or_default());
    }
    if let Some(omitted) = context["native_lines"]["omitted"].as_u64().filter(|n| *n > 0) {
        let _ = writeln!(text, "# Gap: {omitted} other pattern hits not shown; repeat with purpose=locate.");
    }
    for item in live.into_iter().flatten() {
        let file = item["source"]["file"].as_str().unwrap_or_default();
        if !context["pattern_files"].is_null() && listed.contains(&(file, item["source"]["line"].as_u64())) {continue;}
        let _ = writeln!(
            text,
            "@ {file} (current source without an indexed symbol)\n{}",
            item["excerpt"]["text"].as_str().unwrap_or_default()
        );
    }
}

/// The file of a declaration id written as `file:line:name`.
fn declared_file(id: &str) -> &str {
    id.rsplitn(3, ':').nth(2).unwrap_or_default()
}

/// How many files the evidence links to that the pattern did not hit.
fn left_out(context: &Value, pattern: &BTreeSet<&str>) -> usize {
    if pattern.is_empty() {return 0;}
    let sources = [("cards", "source"), ("static_references", "source"), ("resources", "source")].into_iter()
        .flat_map(|(key, source)| context[key].as_array().into_iter().flatten().map(move |item| &item[source]))
        .chain(["steps", "relations"].into_iter().flat_map(|key| context["chain"][key].as_array().into_iter().flatten().map(|item| &item["target_source"])))
        .chain(context["chain"]["test_mentions"].as_array().into_iter().flatten().map(|item| &item["source"]));
    sources.filter_map(|source| source["file"].as_str()).filter(|file| !pattern.contains(file)).collect::<BTreeSet<_>>().len()
}

/// Keep one path header per source file. Only known, indexed headers qualify;
/// opaque/verbatim resources keep their original ordering and representation.
fn coalesce_files(text:String,context:&Value)->String {
    if context["resources"].as_array().is_some_and(|items|!items.is_empty()) {return text;}
    let mut files=BTreeSet::new();
    for key in ["cards","static_references"] {
        for item in context[key].as_array().into_iter().flatten() {
            if let Some(file)=item["source"]["file"].as_str(){files.insert(file);}
        }
    }
    for item in context["native_lines"]["lines"].as_array().into_iter().flatten() {
        if let Some(file)=item["source"]["file"].as_str(){files.insert(file);}
    }
    for key in ["steps","test_mentions"] {
        for item in context["chain"][key].as_array().into_iter().flatten() {
            let source=if key=="steps" {"target_source"}else{"source"};
            if let Some(file)=item[source]["file"].as_str(){files.insert(file);}
        }
    }
    let mut header=String::new();let mut footer=String::new();
    let mut blocks=BTreeMap::<&str,String>::new();let mut order=Vec::new();let mut current=None;let mut started=false;
    for line in text.split_inclusive('\n') {
        if let Some(file)=line.strip_prefix("@ ") {
            let file=file.trim_end_matches('\n');
            if !files.contains(file) {return text;}
            if !blocks.contains_key(file) {order.push(file);}
            blocks.entry(file).or_default();current=Some(file);started=true;continue;
        }
        if ["# Question ","# Deferred ", "# Gap:","# Partial evidence", "# Additional static target", "# Interpretation ", "# static relation: "]
            .iter().any(|prefix|line.starts_with(prefix))
            || line.starts_with("# ") && line.contains("; runtime unverified]: ") {
            current=None;
        }
        if let Some(file)=current {blocks.get_mut(file).expect("known file block").push_str(line);}
        else if started {footer.push_str(line);}else{header.push_str(line);}
    }
    let mut result=header;
    for file in order {let _=writeln!(result,"@ {file}");result.push_str(&blocks[file]);}
    result.push_str(&footer);
    if result.len()<text.len() {result}else{text}
}

/// Group repeated paths and provenance labels without removing graph records.
/// These are source references, never a claim of runtime execution order.
fn graph(text:&mut String,context:&Value,shown:&dyn Fn(&str)->bool) {
    let mut steps=BTreeMap::<&str,Vec<String>>::new();
    for step in context["chain"]["steps"].as_array().into_iter().flatten()
        .filter(|step|shown(step["target_source"]["file"].as_str().unwrap_or_default())) {
        let from=step["from"].as_str().unwrap_or_default().rsplit(':').next().unwrap_or_default();
        let to=step["to"].as_str().unwrap_or_default().rsplit(':').next().unwrap_or_default();
        steps.entry(step["target_source"]["file"].as_str().unwrap_or_default()).or_default().push(format!(
            "{from}:{} -> {to} {}-{}",step["call_source"]["line"],step["target_source"]["line"],step["target_source"]["end_line"]));
    }
    for (file,records) in steps {let _=writeln!(text,"@ {file}\n# Native follow-up: {} [static; effects unverified]",records.join("; "));}
    let mut relations=BTreeMap::<(&str,&str),Vec<String>>::new();
    for relation in context["chain"]["relations"].as_array().into_iter().flatten()
        .filter(|relation|shown(relation["target_source"]["file"].as_str().unwrap_or_default())) {
        let name=|key|relation[key].as_str().unwrap_or_default().rsplit(':').next().unwrap_or_default();
        relations.entry((relation["relation"].as_str().unwrap_or_default(),relation["resolution"].as_str().unwrap_or_default())).or_default()
            .push(format!("{} -> {}",name("from"),name("to")));
    }
    for ((kind,resolution),records) in relations {let _=writeln!(text,"# {kind} [{resolution}; runtime unverified]: {}",records.join("; "));}
}

#[cfg(test)]
mod graph_tests {
    use super::*;
    #[test]
    fn direct_navigation_deduplicates_only_the_same_unique_dependency() {
        let mut context=serde_json::json!({"status":"current-task-evidence","written_clues":{"covered_slots":1},"cards":[
            {"id":"a:1:first","name":"first","source":{"file":"a","line":1,"end_line":2},"initial_source_excerpt":true,"parse_complete":true,"source_excerpt":{"text":"1 | first()\n2 | target()"}}],
            "chain":{"steps":[{"operation":"unique-static-dependency","from":"a:1:first","to":"b:10:target","call_source":{"line":2},"target_source":{"file":"b","line":10,"end_line":15}}]},
            "navigation":{"paths":[{"from":"a:1:first","to":"b:10:target","via":"outgoing","distance":1,"resolution":"unique-static-target"}]}});
        let request=Request {tool:"rg".into(),input:serde_json::json!({"args":["first","."]}),intent:"inspect first".into(),purpose:crate::domain::knowledge::investigation::Purpose::Implement,choose:false};
        let render=|context:&Value|String::from_utf8(agent(&Answer {report:serde_json::json!({"task_context":context}),stdout:vec![],stderr:vec![],exit_code:0},&request).unwrap().stdout).unwrap();
        let view=render(&context);
        assert!(view.contains("first:2 -> target 10-15"));assert!(view.contains("1 | first()"));
        assert!(!view.contains("# static relation:"));
        context["navigation"]["paths"][0]["resolution"]=serde_json::json!("ambiguous");
        assert!(render(&context).contains("# static relation:"));
        context["navigation"]["paths"][0]["resolution"]=serde_json::json!("unique-static-target");
        context["navigation"]["paths"][0]["distance"]=serde_json::json!(2);
        assert!(render(&context).contains("# static relation:"));
    }
    #[test]
    fn pattern_hits_come_before_every_reference_and_files_without_hits_stay_out() {
        let context=serde_json::json!({"status":"current-task-evidence","written_clues":{"covered_slots":1},"pattern_files":["a.ts","c.ts"],"cards":[
            {"id":"b.ts:5:helper","name":"helper","source":{"file":"b.ts","line":5,"end_line":5},"initial_source_excerpt":true,"parse_complete":true,
                "source_excerpt":{"text":"5 | export function helper() { return 2; }","truncated":false}},
            {"id":"a.ts:9:other","name":"other","source":{"file":"a.ts","line":9,"end_line":10},"initial_reference":true},
            {"id":"a.ts:2:entry","name":"entry","source":{"file":"a.ts","line":2,"end_line":2},"initial_source_excerpt":true,"shows_pattern_hit":true,"parse_complete":true,
                "source_excerpt":{"text":"2 | export function quartzEntry() { return helper(); }","truncated":false}}],
            "static_references":[{"id":"b.ts:8:util","name":"util","initial_reference":true,"source":{"file":"b.ts","line":8,"end_line":9}}],
            "chain":{"steps":[{"operation":"unique-static-dependency","from":"a.ts:2:entry","to":"b.ts:5:helper","call_source":{"line":2},"target_source":{"file":"b.ts","line":5,"end_line":5}}]},
            "navigation":{"paths":[{"from":"a.ts:2:entry","to":"b.ts:5:helper","via":"caller","distance":1,"resolution":"ambiguous"}]},
            "resources":[{"source":{"file":"docs/x.md","line":1,"end_line":1},"text":"quartz notes"}],
            "native_lines":{"lines":[{"source":{"file":"c.ts","line":7,"end_line":7},"text":"quartzEntry();"}],"omitted":0}});
        let request=Request{tool:"rg".into(),input:serde_json::json!({"args":["quartzEntry","."]}),intent:"recover the quartz snapshot".into(),
            purpose:crate::domain::knowledge::investigation::Purpose::Implement,choose:false};
        let answer=Answer{report:serde_json::json!({"task_context":context}),stdout:vec![],stderr:vec![],exit_code:0};
        let view=String::from_utf8(agent(&answer,&request).unwrap().stdout).unwrap();
        let reference=view.find("# References").expect("the hit file keeps its references");
        for hit in ["2 | export function quartzEntry()","7 | quartzEntry();"] {
            assert!(view.find(hit).is_some_and(|at|at<reference),"{hit}: {view}");
        }
        assert!(!view.contains("b.ts") && !view.contains("docs/x.md") && !view.contains("helper() { return 2"),"{view}");
        assert!(view.contains("# Files without pattern hits left out: 2;"),"{view}");
    }
    #[test]
    fn file_coalescing_keeps_source_context_references_and_global_gaps() {
        let context=serde_json::json!({"cards":[{"source":{"file":"a.ext"}},{"source":{"file":"b.ext"}}]});
        let text="# task evidence\n@ a.ext\n1 | first\n@ b.ext\n2 | other\n@ a.ext\n# References (expand source/responsibility): target 3-5\n# caller [ambiguous; runtime unverified]: entry -> target\n# Interpretation [reviewed; author assertion]: global hypothesis\n# Gap: unknown runtime\n".to_string();
        let result=coalesce_files(text.clone(),&context);
        assert_eq!(result.matches("@ a.ext").count(),1);
        assert!(result.contains("@ a.ext\n1 | first\n# References"));
        assert!(result.contains("@ b.ext\n2 | other\n# caller [ambiguous; runtime unverified]: entry -> target\n# Interpretation"));
        assert!(result.ends_with("# Gap: unknown runtime\n"));
        let mut opaque=context;opaque["resources"]=serde_json::json!([{"text":"@ a.ext"}]);
        assert_eq!(coalesce_files(text.clone(),&opaque),text);
    }
    #[test]
    fn dependency_destinations_keep_all_coordinates_without_repeating_unrequested_signatures() {
        let context=serde_json::json!({"status":"current-task-evidence","written_clues":{"covered_slots":1},"cards":[
            {"id":"entry","name":"entry","source":{"file":"entry.ext","line":1,"end_line":2,"sha256":"current"},
                "initial_source_excerpt":true,"source_excerpt":{"text":"1 | entry()\n2 | call_target()","truncated":false}}],
            "static_references":(0..50).map(|at|serde_json::json!({"id":format!("target-{at}"),"name":format!("target_{at}"),"initial_reference":true,
                "source":{"file":"targets.ext","line":at+1,"end_line":at+2,"sha256":"current"},"signature":"VeryLongUnrequestedSignature".repeat(20)})).collect::<Vec<_>>()});
        let answer=Answer{report:serde_json::json!({"task_context":context}),stdout:vec![],stderr:vec![],exit_code:0};
        let request=Request{tool:"rg".into(),input:serde_json::json!({"args":["entry","."]}),intent:"inspect entry".into(),
            purpose:crate::domain::knowledge::investigation::Purpose::Spec,choose:false};
        let view=String::from_utf8(agent(&answer,&request).unwrap().stdout).unwrap();
        for at in 0..50 {assert!(view.contains(&format!("target_{at} {}-{}",at+1,at+2)));}
        assert!(!view.contains("VeryLongUnrequestedSignature"));
        assert!(answer.report["task_context"]["static_references"][0]["signature"].as_str().unwrap().contains("VeryLongUnrequestedSignature"));
        assert!(view.contains("1 | entry()"));assert!(view.contains("purpose=locate"));
    }
    #[test]
    fn grouped_graph_keeps_each_source_range_and_each_resolution_without_repeating_paths() {
        let context=serde_json::json!({"chain":{"steps":[
            {"from":"a:1:first","to":"b:10:read","call_source":{"line":2},"target_source":{"file":"long/nested/file.rs","line":10,"end_line":15}},
            {"from":"a:3:second","to":"b:20:write","call_source":{"line":4},"target_source":{"file":"long/nested/file.rs","line":20,"end_line":25}}],
            "relations":[{"from":"a:1:first","to":"b:10:read","relation":"caller","resolution":"ambiguous"},
                {"from":"a:3:second","to":"b:20:write","relation":"caller","resolution":"unique-static-target"}]}});
        let mut view=String::new();graph(&mut view,&context,&|_|true);
        assert_eq!(view.matches("long/nested/file.rs").count(),1);
        for record in ["first:2 -> read 10-15","second:4 -> write 20-25","caller [ambiguous; runtime unverified]: first -> read","caller [unique-static-target; runtime unverified]: second -> write"] {assert!(view.contains(record),"{view}");}
    }
}
