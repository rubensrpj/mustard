#[path = "support/model.rs"]
mod model;

use mustard_core::domain::code_search::Request;
use mustard_core::domain::knowledge::selection::{Ambiguity, Decisions, SymbolSelector};
use mustard_core::io::code_search;
use serde_json::json;
use std::cell::Cell;
use std::path::Path;

fn fixture(scan: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
    std::fs::write(dir.path().join("src/a.rs"),"/// Persist the quartz snapshot.\npub fn first() { let quartz = 1; }\n/// Persist the quartz snapshot.\npub fn second() { let quartz = 2; }\n").unwrap();
    if scan {
        model::scan(dir.path(), &dir.path().join(".claude"), &[]);
    }
    dir
}
fn request(args: &[&str]) -> Request {
    Request::native(&args.iter().map(|v| v.to_string()).collect::<Vec<_>>()).unwrap()
}
fn execute(
    root: &Path,
    request: &Request,
    selector: Option<&dyn SymbolSelector>,
) -> code_search::Answer {
    code_search::execute(root, root, root, request, selector).unwrap()
}

#[test]
fn raw_results_match_real_search_options_with_and_without_a_database() {
    for scan in [false, true] {
        let dir = fixture(scan);
        let root = dir.path();
        for args in [
            vec!["rg", "-n", "--with-filename", "quartz", "src"],
            vec![
                "rg",
                "-n",
                "--with-filename",
                "-i",
                "-w",
                "-F",
                "QUARTZ",
                "src",
            ],
            vec![
                "rg",
                "-n",
                "--with-filename",
                "-C",
                "1",
                "--glob",
                "*.rs",
                "quartz",
                ".",
            ],
            vec!["rg", "-c", "quartz", "src"],
            vec!["rg", "--files", "src"],
            vec!["rg", "-n", "--with-filename", "absent-sentinel", "src"],
        ] {
            let native = std::process::Command::new(args[0])
                .args(&args[1..])
                .current_dir(root)
                .output()
                .unwrap();
            let answer = execute(root, &request(&args), None);
            assert_eq!(answer.stdout, native.stdout, "{args:?}");
            assert_eq!(answer.stderr, native.stderr, "{args:?}");
            assert_eq!(answer.exit_code, native.status.code().unwrap(), "{args:?}");
            assert_eq!(answer.report["remote_model_calls"], 0);
        }
    }
}

#[test]
fn current_owners_include_exact_read_ranges_and_comments() {
    let dir = fixture(true);
    let root = dir.path();
    let answer = execute(
        root,
        &request(&["rg", "-n", "--with-filename", "let quartz", "src"]),
        None,
    );
    assert_eq!(answer.report["crossing_status"], "enriched");
    assert_eq!(
        answer.report["evidence"]["symbols"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(answer.report["evidence"]["intent_requested"], true);
    let symbol = &answer.report["evidence"]["symbols"][0];
    assert!(symbol["documentation"].as_str().unwrap().contains("quartz"));
    assert_eq!(symbol["read"]["input"]["offset"], symbol["source"]["line"]);
    assert_eq!(
        symbol["read"]["input"]["limit"],
        symbol["source"]["end_line"].as_u64().unwrap() - symbol["source"]["line"].as_u64().unwrap()
            + 1
    );
    assert_eq!(symbol["history"]["args"][2], "history");
}

#[test]
fn new_and_changed_source_is_searched_even_when_absent_from_the_scan() {
    let dir = fixture(true);
    let root = dir.path();
    std::fs::write(
        root.join("src/new.rs"),
        "pub fn new_resource() { let live_sentinel = 1; }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/a.rs"),
        "pub fn moved_resource() { let live_sentinel = 2; }\n",
    )
    .unwrap();
    let answer = execute(
        root,
        &request(&["rg", "-n", "--with-filename", "live_sentinel", "src"]),
        None,
    );
    let text = String::from_utf8(answer.stdout).unwrap();
    assert!(text.contains("src/new.rs:1:"));
    assert!(text.contains("src/a.rs:1:"));
    assert_eq!(
        answer.report["evidence"],
        serde_json::Value::Null,
        "old identities must not be borrowed for new code"
    );
    assert_eq!(answer.report["remote_model_calls"], 0);
}

struct Selector {
    calls: Cell<usize>,
}
impl SymbolSelector for Selector {
    fn select(&self, intent: &str, groups: &[Ambiguity]) -> Decisions {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(intent, "persist quartz snapshot");
        assert_eq!(groups.len(), 1);
        Decisions {
            choices: std::iter::once((groups[0].file.clone(), groups[0].candidates[1].id.clone()))
                .collect(),
            usage: json!({"remote_model_calls":1,"status":"fixture-choice"}),
        }
    }
}

#[test]
fn choice_requires_intent_explicit_authorization_and_an_unresolved_native_tie() {
    let dir = fixture(true);
    let root = dir.path();
    let selector = Selector {
        calls: Cell::new(0),
    };
    let mut req = request(&["rg", "-n", "--with-filename", "let quartz", "src"]);
    req.choose = true;
    execute(root, &req, Some(&selector));
    assert_eq!(selector.calls.get(), 0, "without intent no choice");
    req.intent = "persist quartz snapshot".into();
    req.choose = false;
    let original = execute(root, &req, Some(&selector));
    assert_eq!(selector.calls.get(), 0, "without authorization no choice");
    req.choose = true;
    let chosen = execute(root, &req, Some(&selector));
    assert_eq!(selector.calls.get(), 1);
    assert_eq!(
        original.stdout, chosen.stdout,
        "choice cannot filter native occurrences"
    );
    assert_eq!(chosen.report["remote_model_calls"], 1);
    req.intent = "first".into();
    execute(root, &req, Some(&selector));
    assert_eq!(
        selector.calls.get(),
        1,
        "native exact responsibility needs no choice"
    );
}

#[test]
fn typed_tools_apply_paths_ranges_patterns_and_pagination() {
    let dir = fixture(true);
    let root = dir.path();
    let mut req = request(&["rg"]);
    req.tool = "Grep".into();
    req.input = json!({"pattern":"let quartz","path":"src","output_mode":"content","-n":true,"head_limit":1,"offset":1});
    let answer = execute(root, &req, None);
    assert_eq!(answer.report["result"]["numLines"], 1);
    assert!(
        answer.report["result"]["content"]
            .as_str()
            .unwrap()
            .contains(":4:")
    );
    assert_eq!(answer.report["evidence"]["symbols"][0]["name"], "second");
    req.tool = "Read".into();
    req.input = json!({"file_path":"src/a.rs","offset":4,"limit":1});
    let answer = execute(root, &req, None);
    assert_eq!(answer.report["result"]["numLines"], 1);
    assert_eq!(answer.report["evidence"]["symbols"][0]["name"], "second");
    req.tool = "Glob".into();
    req.input = json!({"pattern":"**/*.rs","path":"src"});
    let answer = execute(root, &req, None);
    assert_eq!(answer.report["result"]["filenames"], json!(["src/a.rs"]));
    assert_eq!(
        answer.report["evidence"]["files"][0]["declarations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn unknown_options_and_executable_flags_are_not_silently_reinterpreted() {
    let dir = fixture(false);
    let root = dir.path();
    for req in [
        request(&["rg", "--pre", "touch should-not-exist", "quartz", "src"]),
        request(&["git", "config", "user.name"]),
        Request {
            tool: "Grep".into(),
            input: json!({"pattern":"quartz","future_flag":true}),
            ..request(&["rg"])
        },
    ] {
        assert!(code_search::execute(root, root, root, &req, None).is_err());
    }
    assert!(!root.join("should-not-exist").exists());
}

#[test]
fn discovered_facts_deduplicate_and_replace_changed_source_versions() {
    let dir = fixture(true);
    let root = dir.path();
    let req = request(&["rg", "-n", "--with-filename", "let quartz", "src"]);
    let first = execute(root, &req, None);
    assert_eq!(first.report["learning"]["new_facts"], 2);
    assert_eq!(
        first.report["learning"]["needs_scan"], false,
        "current indexed sources need no scan"
    );
    let again = execute(root, &req, None);
    assert_eq!(again.report["learning"]["new_facts"], 0);
    assert_eq!(again.report["learning"]["reused_facts"], 2);
    std::fs::write(
        root.join("src/a.rs"),
        "pub fn revised() { let quartz = 3; }\n",
    )
    .unwrap();
    let changed = execute(root, &req, None);
    assert_eq!(changed.report["learning"]["new_facts"], 1);
    assert_eq!(changed.report["learning"]["needs_scan"], true);
    let db = mustard_core::io::map_db::MapDb::open(
        &mustard_core::io::project_map::model_path(root),
        root,
        &[],
    )
    .unwrap();
    let facts: i64 = db
        .conn()
        .query_row("SELECT count(*) FROM search_facts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        facts, 1,
        "old source versions do not accumulate as current facts"
    );
    let text: String = db
        .conn()
        .query_row("SELECT text FROM search_facts", [], |row| row.get(0))
        .unwrap();
    assert!(text.contains("revised"));
    model::scan(root, &root.join(".claude"), &["--native"]);
    let current = execute(root, &req, None);
    assert_eq!(current.report["evidence"]["symbols"][0]["name"], "revised");
    assert_eq!(current.report["learning"]["needs_scan"], false);
    let facts: i64 = db
        .conn()
        .query_row("SELECT count(*) FROM search_facts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(facts, 1, "normal scan preserves discovered facts");
}
