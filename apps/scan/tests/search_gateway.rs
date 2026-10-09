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

// Independent decoder: every original occurrence, duplicate and order must
// survive the alternative representation; comments are metadata, not matches.
fn restore_grouped(bytes: &[u8]) -> String {
    let text = std::str::from_utf8(bytes).unwrap();
    let mut file = "";
    let mut output = String::new();
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("@ ") {
            file = path;
        } else if !line.starts_with("# ") {
            assert!(!file.is_empty());
            output.push_str(file);
            output.push(':');
            output.push_str(line);
            output.push('\n');
        }
    }
    if !text.ends_with('\n') { output.pop(); }
    output
}

#[test]
fn agent_view_keeps_small_searches_exact_and_omits_diagnostic_reports() {
    let dir = fixture(true);
    let req = request(&["rg", "-n", "--with-filename", "let quartz = 1", "src"]);
    let answer = execute(dir.path(), &req, None);
    let view = code_search::presentation::agent(&answer, &req, dir.path());
    assert_eq!(view.representation, "native");
    assert_eq!(view.stdout, answer.stdout);
    assert_eq!(answer.report["evidence"]["symbols"].as_array().unwrap().len(), 1);
    assert_eq!(answer.report["learning"]["new_facts"], 1);
    assert!(!String::from_utf8_lossy(&view.stdout).contains("source_hashes"));
}

#[test]
fn agent_view_adds_current_ranges_only_when_lossless_grouping_pays_for_them() {
    let dir = fixture(false);
    let folder = dir.path().join("src/features/persistence/application/services/snapshot-storage");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("store.rs"),"pub fn persist() {\n    let quartz = 1;\n    let quartz = 2;\n    let quartz = 3;\n    let quartz = 4;\n    let quartz = 5;\n}\n").unwrap();
    model::scan(dir.path(), &dir.path().join(".claude"), &[]);
    let req = request(&["rg", "--sort=path", "-n", "--with-filename", "let quartz", "src"]);
    let answer = execute(dir.path(), &req, None);
    let view = code_search::presentation::agent(&answer, &req, dir.path());
    assert_eq!(view.representation, "grouped-current-owners");
    assert!(view.stdout.len() < answer.stdout.len());
    assert!(view.owner_ranges > 0);
    assert!(String::from_utf8_lossy(&view.stdout).contains("persist 1-7"));
    assert_eq!(restore_grouped(&view.stdout).as_bytes(), answer.stdout);
}

#[test]
fn agent_view_preserves_pagination_and_unsupported_native_formats() {
    let dir = fixture(true);
    let root = dir.path();
    let mut req = request(&["rg"]);
    req.tool = "Grep".into();
    req.input = json!({"pattern":"let quartz","path":"src","output_mode":"content","-n":true,"head_limit":1,"offset":1});
    let answer = execute(root, &req, None);
    let view = code_search::presentation::agent(&answer, &req, root);
    let text = String::from_utf8_lossy(&view.stdout);
    assert!(text.contains("src/a.rs"));
    assert!(text.contains("let quartz = 2"));
    assert!(!text.contains("let quartz = 1"),"unpaginated subprocess output must never leak");
    assert!(view.stdout.len() <= answer.report["result"].to_string().len()+1);
    for args in [
        vec!["rg", "-n", "--with-filename", "-C", "1", "let quartz", "src"],
        vec!["rg", "-c", "quartz", "src"],
        vec!["rg", "--json", "quartz", "src"],
        vec!["rg", "[", "src"],
        vec!["rg", "absent-sentinel", "src"],
    ] {
        let req=request(&args);
        let answer=execute(root,&req,None);
        let view=code_search::presentation::agent(&answer,&req,root);
        assert_eq!(view.stdout,answer.stdout,"{args:?}");
    }
    #[cfg(unix)]
    {
        let folder=root.join("ambiguous");
        std::fs::create_dir(&folder).unwrap();
        for name in ["a-very-long-shared-prefix:12:first.rs","a-very-long-shared-prefix:13:second.rs","a-very-long-shared-prefix:14:third.rs"] {
            std::fs::write(folder.join(name),"pub fn marker() {}\n").unwrap();
        }
        let req=request(&["rg","--files","ambiguous"]);
        let answer=execute(root,&req,None);
        assert_eq!(code_search::presentation::agent(&answer,&req,root).stdout,answer.stdout,"file-list names must never become content coordinates");
    }
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
            choices: std::iter::once((groups[0].key.clone(), groups[0].candidates[1].id.clone()))
                .collect(),
            usage: json!({"remote_model_calls":1,"status":"fixture-choice"}),
            ..Default::default()
        }
    }
}

#[test]
fn choice_requires_intent_explicit_authorization_and_unresolved_responsibility() {
    let dir = fixture(true);
    let root = dir.path();
    let selector = Selector {
        calls: Cell::new(0),
    };
    let mut req = request(&["rg", "-n", "--with-filename", "let quartz", "src"]);
    req.choose = true;
    let error = code_search::execute(root, root, root, &req, Some(&selector)).err().unwrap();
    assert!(error.starts_with("search-intent-required"));
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
    let view = code_search::presentation::agent(&chosen, &req, root);
    assert!(String::from_utf8_lossy(&view.stdout).contains("second 4-4 [recommended]"));
    assert_eq!(restore_grouped(&view.stdout).as_bytes(), chosen.stdout);
    req.intent = "first".into();
    execute(root, &req, Some(&selector));
    assert_eq!(
        selector.calls.get(),
        1,
        "native exact responsibility needs no choice"
    );
}

#[test]
fn responsibility_crosses_languages_and_preserves_a_lexically_weaker_alternative() {
    struct AcrossFiles;
    impl SymbolSelector for AcrossFiles {
        fn select(&self, _: &str, groups: &[Ambiguity]) -> Decisions {
            assert_eq!(groups.len(),1);
            let group=&groups[0];
            assert!(group.candidates.iter().any(|c|c.source.file.ends_with(".rs")));
            let selected=group.candidates.iter().find(|c|c.source.file.ends_with(".py")).unwrap();
            let evidence=&group.excerpts[&selected.id];
            assert!(evidence.complete);
            assert!(evidence.text.contains("return quartz"));
            Decisions {choices:std::iter::once((group.key.clone(),selected.id.clone())).collect(),
                usage:json!({"remote_model_calls":1}),..Default::default()}
        }
    }
    let dir=fixture(false);let root=dir.path();
    std::fs::write(root.join("src/a.rs"),"/// Persist and restore the quartz snapshot archive.\npub fn archive() { let quartz = 1; }\n").unwrap();
    std::fs::write(root.join("src/b.py"),"def restore():\n    quartz = 2\n    return quartz\n").unwrap();
    model::scan(root,&root.join(".claude"),&[]);
    let mut req=request(&["rg","--sort=path","-n","--with-filename","quartz","src"]);
    req.intent="persist restore quartz snapshot archive".into();
    let native=execute(root,&req,None);
    assert!(native.report["evidence"]["recommended_symbols"].as_array().unwrap().is_empty(),"lexical superiority is not proof");
    req.choose=true;
    let selected=execute(root,&req,Some(&AcrossFiles));
    assert_eq!(selected.stdout,native.stdout);
    assert_eq!(selected.report["evidence"]["symbols"][0]["name"],"restore");
    assert_eq!(selected.report["evidence"]["remaining_ambiguities"],0);
}

#[test]
fn file_discovery_reports_breadth_without_changing_the_typed_result() {
    let dir=fixture(true);let root=dir.path();
    let mut req=request(&["rg"]);req.tool="Grep".into();
    req.input=json!({"pattern":"quartz","path":"src","output_mode":"files_with_matches"});
    let answer=execute(root,&req,None);
    assert_eq!(answer.report["query_quality"]["returned_files"],1);
    assert_eq!(answer.report["query_quality"]["native_query_rewritten"],false);
    let view=code_search::presentation::agent(&answer,&req,root);
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&view.stdout).unwrap(),answer.report["result"]);
}

#[test]
fn requested_abstention_is_visible_and_does_not_remove_native_matches() {
    use mustard_core::domain::knowledge::selection::Outcome;
    struct Abstain;
    impl SymbolSelector for Abstain {
        fn select(&self,_:&str,groups:&[Ambiguity])->Decisions {
            Decisions {outcomes:std::iter::once((groups[0].key.clone(),Outcome::NoMatch)).collect(),
                usage:json!({"remote_model_calls":1,"status":"fixture-choice"}),..Default::default()}
        }
    }
    let dir=fixture(true);let root=dir.path();
    let mut req=request(&["rg","-n","--with-filename","let quartz","src"]);
    req.intent="persist quartz snapshot".into();req.choose=true;
    let answer=execute(root,&req,Some(&Abstain));
    assert!(answer.report["evidence"]["recommended_symbols"].as_array().unwrap().is_empty());
    assert_eq!(answer.report["evidence"]["remaining_ambiguities"],0);
    let view=code_search::presentation::agent(&answer,&req,root);
    assert!(String::from_utf8_lossy(&view.stdout).contains("# selection: no-match"));
    assert_eq!(restore_grouped(&view.stdout).as_bytes(),answer.stdout);
    req.choose=false;
    let view=code_search::presentation::agent(&answer,&req,root);
    assert!(!String::from_utf8_lossy(&view.stdout).contains("# selection:"));
    assert!(view.stdout.len()<=answer.stdout.len());
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
    let view = code_search::presentation::agent(&answer, &req, root);
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&view.stdout).unwrap(), answer.report["result"]);
    req.tool = "Glob".into();
    req.input = json!({"pattern":"**/*.rs","path":"src"});
    let answer = execute(root, &req, None);
    assert_eq!(answer.report["result"]["filenames"], json!(["src/a.rs"]));
    let view = code_search::presentation::agent(&answer, &req, root);
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&view.stdout).unwrap(), answer.report["result"]);
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
