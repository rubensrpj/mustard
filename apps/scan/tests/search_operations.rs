#[path = "support/model.rs"]
mod model;
use mustard_core::domain::code_search::Request;
use mustard_core::domain::knowledge::investigation::Purpose;
use mustard_core::io::code_search;
use serde_json::{Value, json};
use std::path::Path;
fn request(tool: &str, input: Value) -> Request {
    Request {
        tool: tool.into(),
        input,
        intent: "Verify current structural connections".into(),
        purpose: Purpose::Understand,
        choose: false,
    }
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"),"pub fn entry() { middle(); }\npub fn middle() { tail(); }\npub fn tail() {}\npub fn elsewhere() {}\n").unwrap();
    model::scan(dir.path(), &dir.path().join(".claude"), &["--native"]);
    dir
}
fn run(root: &Path, req: &Request) -> code_search::Answer {
    code_search::execute(root, root, root, req, None).unwrap()
}
#[test]
fn trace_returns_current_chain_and_target_path_without_unrelated_bodies() {
    let dir = fixture();
    let root = dir.path();
    let req = request(
        "Trace",
        json!({"file_path":"src/lib.rs","symbol":"src/lib.rs:1:entry","target":"src/lib.rs:3:tail","depth":3}),
    );
    let answer = run(root, &req);
    let result = &answer.report["result"];
    assert_eq!(result["target_reached"], true, "{result}");
    assert_eq!(result["paths"].as_array().unwrap().len(), 2);
    assert_eq!(result["cards"].as_array().unwrap().len(), 3);
    assert!(!result.to_string().contains("elsewhere"));
    assert_eq!(answer.report["remote_model_calls"], 0);
    let mut short = req.clone();
    short.input["depth"] = json!(1);
    let short = run(root, &short);
    assert_eq!(short.report["result"]["target_reached"], false);
    assert!(
        short.report["result"]["navigation"]["omitted_destinations"]
            .as_u64()
            .unwrap()
            > 0
    );
    std::fs::write(root.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
    assert!(code_search::execute(root, root, root, &req, None).is_err());
}
#[test]
fn symbol_identity_must_belong_to_explicit_source_and_structure_is_syntax_not_text() {
    let dir = fixture();
    let root = dir.path();
    let bad = request(
        "Symbol",
        json!({"file_path":"src/lib.rs","symbol":"elsewhere.rs:1:entry"}),
    );
    assert!(
        code_search::execute(root, root, root, &bad, None)
            .unwrap_err_string()
            .contains("outside")
    );
}
trait ErrorText {
    fn unwrap_err_string(self) -> String;
}
impl ErrorText for Result<code_search::Answer, String> {
    fn unwrap_err_string(self) -> String {
        match self {
            Err(e) => e,
            Ok(_) => panic!("expected refusal"),
        }
    }
}
#[test]
fn structural_query_runs_on_multiple_languages_and_discloses_parse_and_output_gaps() {
    let dir = fixture();
    let root = dir.path();
    std::fs::write(
        root.join("src/calls.ts"),
        "// target() in comment\nconst text = 'target()';\nfunction work() { target(); }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/calls.py"),
        "# target() in comment\ndef work():\n    target()\n",
    )
    .unwrap();
    for (file, query) in [
        (
            "src/lib.rs",
            "(call_expression function: (identifier) @callee)",
        ),
        (
            "src/calls.ts",
            "(call_expression function: (identifier) @callee)",
        ),
        ("src/calls.py", "(call function: (identifier) @callee)"),
    ] {
        let req = request("Structure", json!({"file_path":file,"query":query}));
        let answer = run(root, &req);
        let result = &answer.report["result"];
        let found = result["matches"].as_array().unwrap();
        assert_eq!(
            found.len(),
            if file.ends_with(".rs") { 2 } else { 1 },
            "{result}"
        );
        assert_eq!(result["parse_complete"], true);
        assert_eq!(result["search_complete"], true);
        assert_eq!(answer.report["local_model_calls"], 0);
        assert!(answer.report["learning"]["new_facts"].as_u64().unwrap() > 0);
    }
    std::fs::write(root.join("src/bad.rs"), "fn incomplete( {\n").unwrap();
    let req = request(
        "Structure",
        json!({"file_path":"src/bad.rs","query":"(ERROR) @error"}),
    );
    assert_eq!(run(root, &req).report["result"]["parse_complete"], false);
    let bad = request(
        "Structure",
        json!({"file_path":"src/lib.rs","query":"(unknown_node) @bad"}),
    );
    assert!(code_search::execute(root, root, root, &bad, None).is_err());
    let huge = (0..140).map(|_| "fn x() {}\n").collect::<String>();
    std::fs::write(root.join("src/many.rs"), huge).unwrap();
    let req = request(
        "Structure",
        json!({"file_path":"src/many.rs","query":"(function_item) @fn"}),
    );
    assert_eq!(run(root, &req).report["result"]["search_complete"], false);
}
#[cfg(unix)]
#[test]
fn advanced_operations_refuse_secret_files_and_symlinks_outside_the_tree() {
    let dir = fixture();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("private.rs"), "fn secret() {}\n").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("private.rs"),
        root.join("src/escape.rs"),
    )
    .unwrap();
    std::fs::write(root.join(".env"), "password=test\n").unwrap();
    for file in ["src/escape.rs", ".env"] {
        let req = request("Structure", json!({"file_path":file,"query":"(_) @any"}));
        assert!(code_search::execute(root, root, root, &req, None).is_err());
    }
}

#[test]
fn unavailable_precision_returns_actual_native_candidates_without_calling_a_model() {
    let dir = fixture();
    let root = dir.path();
    // A scanned language without a server in CODE_TOOLS exercises fallback
    // without relying on the developer's installed tools or network.
    std::fs::write(
        root.join("src/Example.java"),
        "class Example { void calculate() {} }\n",
    )
    .unwrap();
    let req = request(
        "References",
        json!({"file_path":"src/Example.java","line":1,"column":23,"relation":"references"}),
    );
    let answer = run(root, &req);
    let result = &answer.report["result"];
    assert!(result["status"].as_str().unwrap().contains("unavailable"));
    let native = code_search::execute_native(
        root,
        &serde_json::from_value(result["native_fallback"]["request"].clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        result["native_fallback"]["stdout"],
        String::from_utf8(native.stdout).unwrap()
    );
    assert!(
        result["native_fallback"]["stdout"]
            .as_str()
            .unwrap()
            .contains("calculate")
    );
    assert_eq!(answer.report["remote_model_calls"], 0);
    let malformed = request(
        "References",
        json!({"file_path":"src/Example.java","line":1,"column":23,"relation":42}),
    );
    assert!(code_search::execute(root, root, root, &malformed, None).is_err());
}

#[test]
fn accepted_wave_conclusions_are_reused_until_any_supporting_source_changes() {
    use mustard_core::io::knowledge::{self, waves};
    let dir = fixture();
    let root = dir.path();
    std::fs::write(root.join("src/other.rs"), "pub fn related() {}\n").unwrap();
    model::scan(root, &root.join(".claude"), &["--native"]);
    let sources = ["src/lib.rs", "src/other.rs"].map(|file| {
        let mut hash = mustard_core::io::sha256::Sha256::new();
        hash.update(&std::fs::read(root.join(file)).unwrap());
        json!({"file":file,"line":1,"end_line":1,"sha256":hash.hex_digest()})
    });
    let notes = json!([{"title":"Recover quartz snapshot","text":"Entry delegates recovery through middle; inspect both sources.","sources":sources}]);
    assert_eq!(
        waves::capture(root, root, "x", 1, false, &notes).unwrap()["recorded"],
        0
    );
    assert!(knowledge::interpretations(root).unwrap().is_empty());
    assert_eq!(
        waves::capture(root, root, "x", 1, true, &notes).unwrap()["recorded"],
        1
    );
    let again = waves::capture(root, root, "x", 1, true, &notes).unwrap();
    assert_eq!(again["notes"][0]["reused"], true);
    assert_eq!(knowledge::interpretations(root).unwrap().len(), 1);
    let query = || {
        knowledge::query(root, "Recover quartz snapshot", None, 10, 1, false)
            .unwrap()
            .0
    };
    assert_eq!(query()["interpretations"].as_array().unwrap().len(), 1);
    std::fs::write(root.join("src/other.rs"), "pub fn changed() {}\n").unwrap();
    assert!(query()["interpretations"].as_array().unwrap().is_empty());
    assert!(waves::capture(root, root, "x", 2, true, &notes).is_err());
    assert_eq!(knowledge::interpretations(root).unwrap().len(), 1);
}
