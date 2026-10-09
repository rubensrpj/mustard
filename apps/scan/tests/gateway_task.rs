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
