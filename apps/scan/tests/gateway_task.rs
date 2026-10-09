#[path = "support/model.rs"]
mod model;

use mustard_core::domain::code_search::Request;
use mustard_core::domain::knowledge::investigation::Purpose;
use mustard_core::domain::knowledge::selection::{Ambiguity, Decisions, Outcome, SymbolSelector};
use mustard_core::io::code_search;
use serde_json::{Value, json};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/allowed")).unwrap();
    std::fs::create_dir_all(dir.path().join("src/outside")).unwrap();
    std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
    std::fs::write(
        dir.path().join("src/allowed/entry.rs"),
        "pub fn entry() { let entry_sentinel=1; restore(); }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/allowed/store.rs"),"/// Recover the quartz snapshot.\npub fn restore() {\n    let quartz_snapshot=2;\n}\n/// Serialize the quartz snapshot.\npub fn encode() { let quartz_snapshot=3; }\n").unwrap();
    std::fs::write(
        dir.path().join("src/outside/secret.rs"),
        "/// Recover the quartz snapshot.\npub fn outside_sentinel() {}\n",
    )
    .unwrap();
    model::scan(dir.path(), &dir.path().join(".claude"), &[]);
    dir
}
fn request() -> Request {
    Request {
        tool: "rg".into(),
        input: json!({"args":["-n","--with-filename","entry_sentinel","src/allowed"]}),
        intent: "recover quartz snapshot".into(),
        purpose: Purpose::Spec,
        choose: false,
    }
}
fn run(
    root: &Path,
    request: &Request,
    selector: Option<&dyn SymbolSelector>,
) -> code_search::Answer {
    code_search::execute(root, root, root, request, selector).unwrap()
}

#[test]
fn intent_discovers_complementary_functions_and_returns_current_code_not_a_diagnostic_envelope() {
    let dir = fixture();
    let root = dir.path();
    let req = request();
    let answer = run(root, &req, None);
    assert_eq!(
        answer.report["task_context"]["status"], "current-task-evidence",
        "{}",
        answer.report["task_context"]
    );
    let cards = answer.report["task_context"]["cards"].as_array().unwrap();
    assert!(cards.iter().any(|card| card["name"] == "restore"));
    assert!(!String::from_utf8_lossy(&answer.stdout).contains("quartz_snapshot"));
    assert!(cards.iter().all(|card| {
        card["source"]["file"]
            .as_str()
            .unwrap()
            .starts_with("src/allowed/")
    }));
    let view = code_search::presentation::agent(&answer, &req, root);
    assert_eq!(view.representation, "current-task-evidence");
    let text = String::from_utf8(view.stdout).unwrap();
    assert!(text.contains("let quartz_snapshot=2"));
    assert!(text.contains("purpose=locate"));
    assert!(!text.contains("outside_sentinel"));
    assert!(!text.contains("hydrated_candidates"));
    assert_eq!(answer.report["remote_model_calls"], 0);
    assert!(
        answer.report["task_context"]["learning"]["new_facts"]
            .as_u64()
            .unwrap()
            > 0
    );
    let repeat = run(root, &req, None);
    assert_eq!(repeat.report["task_context"]["learning"]["new_facts"], 0);
    let locate = run(
        root,
        &Request {
            purpose: Purpose::Locate,
            ..req
        },
        None,
    );
    assert_eq!(locate.stdout, answer.stdout);
    assert!(locate.report.get("task_context").is_none());
}

#[test]
fn file_discovery_and_globs_scope_before_ranking_and_preserve_the_original_page() {
    let dir = fixture();
    let root = dir.path();
    let req = Request {
        tool: "Grep".into(),
        input: json!({"pattern":"pub","path":"src/allowed","glob":"*store.rs","output_mode":"files_with_matches","head_limit":1}),
        ..request()
    };
    let answer = run(root, &req, None);
    assert_eq!(answer.report["result"]["numFiles"], 1);
    assert_eq!(answer.report["task_context"]["scope"]["files"], 1);
    assert!(
        answer.report["task_context"]["cards"]
            .as_array()
            .unwrap()
            .iter()
            .all(|card| card["source"]["file"] == "src/allowed/store.rs")
    );
    let view =
        String::from_utf8(code_search::presentation::agent(&answer, &req, root).stdout).unwrap();
    assert!(view.contains("Original page:"));
    assert!(!view.contains("outside_sentinel"));
}

#[test]
fn read_counts_unknown_scope_options_and_missing_index_keep_the_native_contract() {
    let dir = fixture();
    let root = dir.path();
    let req = Request {
        tool: "Read".into(),
        input: json!({"file_path":"src/allowed/store.rs","offset":2,"limit":1}),
        ..request()
    };
    let answer = run(root, &req, None);
    assert!(answer.report.get("task_context").is_none());
    let view: Value =
        serde_json::from_slice(&code_search::presentation::agent(&answer, &req, root).stdout)
            .unwrap();
    assert_eq!(view, answer.report["result"]);
    let req = Request {
        tool: "Grep".into(),
        input: json!({"pattern":"quartz","path":"src","output_mode":"count"}),
        ..request()
    };
    let answer = run(root, &req, None);
    assert!(answer.report.get("task_context").is_none());
    let req = Request {
        input: json!({"args":["-n","--with-filename","--max-count","1","entry_sentinel","src/allowed"]}),
        ..request()
    };
    let answer = run(root, &req, None);
    assert_eq!(answer.report["task_context"]["status"], "native-fallback");
    let native = std::process::Command::new("rg")
        .args([
            "-n",
            "--with-filename",
            "--max-count",
            "1",
            "entry_sentinel",
            "src/allowed",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert_eq!(answer.stdout, native.stdout);
    let empty = tempfile::tempdir().unwrap();
    std::fs::write(empty.path().join("a.txt"), "needle\n").unwrap();
    let req = Request {
        input: json!({"args":["-n","--with-filename","needle","."]}),
        ..request()
    };
    let answer = run(empty.path(), &req, None);
    assert!(
        answer.report["task_context"]["cards"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
    assert_ne!(
        code_search::presentation::agent(&answer, &req, empty.path()).representation,
        "current-task-evidence"
    );
    assert!(!answer.stdout.is_empty());
}

#[test]
fn unmatched_intent_does_not_replace_a_useful_native_result_with_only_references() {
    let dir = fixture();
    let req = Request {
        intent: "unrelatedSaffronNebula".into(),
        ..request()
    };
    let answer = run(dir.path(), &req, None);
    let view = code_search::presentation::agent(&answer, &req, dir.path());
    assert_ne!(view.representation, "current-task-evidence");
    assert!(
        String::from_utf8(view.stdout)
            .unwrap()
            .contains("entry_sentinel")
    );
    assert_eq!(answer.report["remote_model_calls"], 0);
}

#[test]
fn a_named_native_declaration_keeps_its_body_and_defers_unrelated_word_matches() {
    let dir=fixture();
    let root=dir.path();
    std::fs::write(root.join("src/allowed/catalog.ts"),"export class Catalog {\n  async retrieve() {\n    return this.database.findMany({ snapshots: true });\n  }\n  formatReport() {\n    return { archive: true };\n  }\n}\n").unwrap();
    model::scan(root,&root.join(".claude"),&[]);
    let req=Request {
        input:json!({"args":["-n","--with-filename","retrieve","src/allowed/catalog.ts"]}),
        intent:"retrieve database snapshots and archive output".into(),
        ..request()
    };
    let answer=run(root,&req,None);
    let cards=answer.report["task_context"]["cards"].as_array().unwrap();
    let target=cards.iter().find(|card|card["name"]=="retrieve").unwrap();
    assert_eq!(target["initial_source_excerpt"],true);
    assert_eq!(target["source_excerpt"]["truncated"],false);
    assert_eq!(target["missing_source_reads"],json!([]));
    let lateral=cards.iter().find(|card|card["name"]=="formatReport").unwrap();
    assert_eq!(lateral["initial_source_excerpt"],false);
    let view=String::from_utf8(code_search::presentation::agent(&answer,&req,root).stdout).unwrap();
    assert!(view.contains("this.database.findMany"));
    assert!(!view.contains("return { archive: true }"));
    assert!(view.contains("additional candidates"));
    assert!(view.contains("run map summary --file"));
    let native=run(root,&Request{purpose:Purpose::Locate,..req},None);
    assert_eq!(answer.stdout,native.stdout);
    assert_eq!(answer.report["remote_model_calls"],0);
}

#[test]
fn relations_outside_the_requested_file_are_not_reported_as_stale_sources() {
    let dir=fixture();
    let root=dir.path();
    let req=Request {
        input:json!({"args":["-n","--with-filename","entry_sentinel","src/allowed/entry.rs"]}),
        ..request()
    };
    let answer=run(root,&req,None);
    let navigation=&answer.report["task_context"]["navigation"];
    assert!(navigation["outside_scope_relations"].as_u64().unwrap()>0,"{navigation}");
    assert_eq!(navigation["stale_relations"],0);
    assert!(!answer.report["task_context"]["gaps"].as_array().unwrap().iter().filter_map(Value::as_str).any(|gap|gap.contains("source changed")));
    // Even an actually changed excluded target must not be read to classify
    // the scoped query's current relations.
    std::fs::write(root.join("src/allowed/store.rs"),"fn replaced() {}\n").unwrap();
    let changed=run(root,&req,None);
    assert_eq!(changed.report["task_context"]["navigation"]["stale_relations"],0);
}

#[test]
fn named_source_follows_unique_callee_and_keeps_explicit_read_available() {
    let dir=fixture();let root=dir.path();
    std::fs::write(root.join("src/allowed/chain.rs"),"pub fn read_current() { fetch(); }\npub fn fetch() { let quartz_snapshot=3; }\n").unwrap();
    model::scan(root,&root.join(".claude"),&[]);
    let req=Request{input:json!({"args":["-n","--with-filename","read_current","src/allowed/chain.rs"]}),..request()};
    let answer=run(root,&req,None);
    let target=answer.report["task_context"]["cards"].as_array().unwrap().iter().find(|card|card["name"]=="fetch").unwrap();
    assert_eq!(target["initial_source_excerpt"],true);
    assert_eq!(target["initial_reference"],true);
    let view=String::from_utf8(code_search::presentation::agent(&answer,&req,root).stdout).unwrap();
    assert!(view.contains("let quartz_snapshot=3"));
    assert!(view.contains("# Native follow-up:"));
    let read=Request{tool:"Read".into(),input:target["read"]["input"].clone(),..req};
    assert!(run(root,&read,None).report["result"]["content"].as_str().unwrap().contains("let quartz_snapshot=3"));
}

#[test]
fn incomplete_bodies_request_only_unseen_ranges_including_cropped_source_lines() {
    let dir=fixture();
    let root=dir.path();
    let body=(0..100).map(|at|format!("    let quartz_snapshot_{at}=1;\n")).collect::<String>();
    let text=format!("pub fn restore() {{\n{body}    let quartz_snapshot_long=\"{}\";\n}}\n","á".repeat(600));
    std::fs::write(root.join("src/allowed/long.rs"),&text).unwrap();
    model::scan(root,&root.join(".claude"),&[]);
    let req=Request {
        input:json!({"args":["-n","--with-filename","restore","src/allowed/long.rs"]}),
        purpose:Purpose::Understand,
        ..request()
    };
    let answer=run(root,&req,None);
    let card=answer.report["task_context"]["cards"].as_array().unwrap().iter().find(|card|card["name"]=="restore").unwrap();
    assert_eq!(card["initial_source_excerpt"],true);
    assert_eq!(card["source_excerpt"]["truncated"],true);
    let source:Vec<_>=text.lines().collect();
    let excerpt=&card["source_excerpt"];
    let mut received=std::collections::BTreeSet::new();
    for row in excerpt["text"].as_str().unwrap().lines() {
        let (line,value)=row.split_once(" | ").unwrap();
        let line:usize=line.parse().unwrap();
        if source[line-1]==value {received.insert(line);}
    }
    let initial=received.clone();
    for expansion in card["missing_source_reads"].as_array().unwrap() {
        let read=Request{tool:"Read".into(),input:expansion["input"].clone(),..req.clone()};
        let result=run(root,&read,None).report["result"].clone();
        for (at,value) in result["content"].as_str().unwrap().lines().enumerate() {
            let line=result["offset"].as_u64().unwrap() as usize+at;
            assert_eq!(source[line-1],value);
            assert!(!initial.contains(&line),"Complete source must not be read twice");
            received.insert(line);
        }
    }
    assert_eq!(received.len(),source.len());
    let view=String::from_utf8(code_search::presentation::agent(&answer,&req,root).stdout).unwrap();
    assert!(view.contains("Missing source ranges"));
    assert!(!view.contains("Read this file at offset"));
}

#[test]
fn newly_discovered_source_without_git_is_live_evidence_and_never_borrows_an_old_symbol() {
    let dir = fixture();
    let root = dir.path();
    std::fs::write(
        root.join("src/allowed/new.py"),
        "def fresh():\n    return 'recover quartz snapshot fresh_sentinel'\n",
    )
    .unwrap();
    let answer = run(root, &request(), None);
    let live = answer.report["task_context"]["investigation"]["live_matches"]
        .as_array()
        .unwrap();
    assert!(
        live.iter()
            .any(|item| item["source"]["file"] == "src/allowed/new.py"
                && item["indexed_symbol"].is_null())
    );
    let view =
        String::from_utf8(code_search::presentation::agent(&answer, &request(), root).stdout)
            .unwrap();
    assert!(view.contains("fresh_sentinel"));
    assert_eq!(
        answer.report["task_context"]["learning"]["needs_scan"],
        true
    );
}

struct Progressive {
    calls: Cell<usize>,
    mutate: Option<std::path::PathBuf>,
}

#[test]
fn a_unique_literal_native_name_needs_no_paid_choice_for_a_longer_question() {
    let dir=fixture();
    let req=Request {
        input:json!({"args":["-n","--with-filename","restore","src/allowed/store.rs"]}),
        intent:"How does restore recover the quartz snapshot?".into(),
        choose:true,
        ..request()
    };
    let selector=Progressive{calls:Cell::new(0),mutate:None};
    let answer=run(dir.path(),&req,Some(&selector));
    assert_eq!(selector.calls.get(),0);
    assert_eq!(answer.report["remote_model_calls"],0);
    assert_eq!(answer.report["task_context"]["selection_basis"],"exact-symbol-identity");
    let selected=answer.report["task_context"]["cards"].as_array().unwrap().iter().find(|card|card["name"]=="restore").unwrap();
    assert_eq!(selected["recommended"],true);
    std::fs::write(dir.path().join("src/allowed/other.py"),"def restore():\n    return 'recover quartz snapshot'\n").unwrap();
    model::scan(dir.path(),&dir.path().join(".claude"),&[]);
    let homonyms=run(dir.path(),&Request{input:json!({"args":["-n","--with-filename","restore","src/allowed"]}),..req},Some(&selector));
    assert!(selector.calls.get()>0,"Homonyms require a comparison or abstention");
    assert_ne!(homonyms.report["task_context"]["selection_basis"],"exact-symbol-identity");
    std::fs::write(dir.path().join("src/allowed/store.rs"),"// restore is mentioned outside a declaration\npub fn restore() { let quartz_snapshot=2; }\npub fn encode() { let quartz_snapshot=3; }\n").unwrap();
    model::scan(dir.path(),&dir.path().join(".claude"),&[]);
    let incomplete=run(dir.path(),&Request{input:json!({"args":["-n","--with-filename","restore","src/allowed/store.rs"]}),..request()},None);
    assert!(incomplete.report["evidence"]["unmapped_occurrences"].as_u64().unwrap()>0);
    assert_eq!(incomplete.report["task_context"]["source_focus"]["native_crossing_complete"],false);
    assert_ne!(incomplete.report["task_context"]["selection_basis"],"exact-symbol-identity");
}
impl SymbolSelector for Progressive {
    fn select(&self, _: &str, groups: &[Ambiguity]) -> Decisions {
        let call = self.calls.get() + 1;
        self.calls.set(call);
        if let Some(path) = &self.mutate {
            std::fs::write(path, "pub fn changed() {}\n").unwrap();
        }
        let group = &groups[0];
        let target = group
            .candidates
            .iter()
            .find(|card| card.name == "restore")
            .unwrap();
        let complete = group.excerpts[&target.id].complete;
        let outcome = if complete {
            Outcome::Selected
        } else {
            Outcome::InsufficientEvidence
        };
        Decisions {
            choices: if complete {
                BTreeMap::from([(group.key.clone(), target.id.clone())])
            } else {
                BTreeMap::new()
            },
            outcomes: BTreeMap::from([(group.key.clone(), outcome)]),
            usage: json!({"status":"fixture-choice","remote_model_calls":1,"input_tokens":10,"known_input_tokens":10,"cost_micro_usd":1,"usage_complete":true}),
        }
    }
}

#[test]
fn explicit_insufficiency_expands_current_source_once_and_accounts_for_both_choices() {
    let dir = fixture();
    let root = dir.path();
    let body = (0..90)
        .map(|at| format!("    let quartz_snapshot_{at}=1;\n"))
        .collect::<String>();
    std::fs::write(root.join("src/allowed/store.rs"),format!("/// Recover the quartz snapshot.\npub fn restore() {{\n{body}}}\n/// Recover the quartz snapshot.\npub fn encode() {{}}\n")).unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let selector = Progressive {
        calls: Cell::new(0),
        mutate: None,
    };
    let req = Request {
        choose: true,
        ..request()
    };
    let answer = run(root, &req, Some(&selector));
    assert_eq!(selector.calls.get(), 2, "{}", answer.report["task_context"]);
    assert_eq!(answer.report["remote_model_calls"], 2);
    assert_eq!(
        answer.report["task_context"]["selection"]["input_tokens"],
        20
    );
    assert_eq!(
        answer.report["task_context"]["selection"]["outcomes"]["responsibility"],
        "selected"
    );
    let ordinary = run(root, &request(), Some(&selector));
    assert_eq!(ordinary.report["remote_model_calls"], 0);
    assert_eq!(selector.calls.get(), 2);
}

#[test]
fn source_changes_during_selection_discard_task_evidence_but_keep_native_matches_and_physical_usage()
 {
    let dir = fixture();
    let root = dir.path();
    let selector = Progressive {
        calls: Cell::new(0),
        mutate: Some(root.join("src/allowed/store.rs")),
    };
    let req = Request {
        choose: true,
        ..request()
    };
    let answer = run(root, &req, Some(&selector));
    assert_eq!(
        answer.report["task_context"]["status"],
        "discarded-source-changed"
    );
    assert_eq!(answer.report["remote_model_calls"], 1);
    assert!(String::from_utf8_lossy(&answer.stdout).contains("entry_sentinel"));
    assert_ne!(
        code_search::presentation::agent(&answer, &req, root).representation,
        "current-task-evidence"
    );
}

#[test]
fn exact_named_anchor_defers_generic_unindexed_mocks_but_preserves_native_hits() {
    let dir = fixture();
    let root = dir.path();
    std::fs::write(
        root.join("src/allowed/noise.test.rs"),
        "// create a file for an unrelated snapshot\nfn dummy_mock() {}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/allowed/actual.test.rs"),
        "// entry is exercised here\nfn exercise() { entry(); }\n",
    )
    .unwrap();
    let mut req = request();
    req.intent = "create a file for snapshot".into();
    for pattern in ["entry", "entry|restore"] {
        req.input = json!({"args":["-n","--with-filename","--sort=path",pattern,"src/allowed"]});
        let answer = run(root, &req, None);
        let native = String::from_utf8_lossy(&answer.stdout);
        assert!(native.contains("actual.test.rs"));
        let investigation = &answer.report["task_context"]["investigation"];
        assert!(investigation["live_matches"].is_array(), "{investigation}");
        assert!(
            investigation["phases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|phase| phase["weak_complements_deferred"]
                    .as_u64()
                    .is_some_and(|count| count > 0)),
            "{investigation}"
        );
        let live = investigation["live_matches"].to_string();
        assert!(!live.contains("noise.test.rs"), "{live}");
        assert_eq!(answer.report["remote_model_calls"], 0);
        let locate = code_search::execute_native(root, &req).unwrap();
        assert_eq!(answer.stdout, locate.stdout);
    }
}
