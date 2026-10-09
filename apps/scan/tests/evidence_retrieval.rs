#[path = "support/model.rs"]
mod model;
use mustard_core::io::{knowledge, project_map as store};
use serde_json::json;

#[test]
fn body_identifiers_locate_functions_across_grammars_with_compact_source_witnesses() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR","code":"en-US"},"ai":{"fallback":false,"vectors":false}}"#).unwrap();
    for (file, source) in [
        ("duration.rs", "pub fn trace_event(request: &str) { let duration_ms = 12; logger(request, duration_ms); }\n"),
        ("headers.py", "def accept_message(request):\n    return request.headers['request-proof-key']\n"),
        ("response.ts", "export function rejectMessage(error: Error, response: any) { return response.status(422).json({error: error.message}); }\n"),
    ] {
        std::fs::write(root.join("src").join(file), source).unwrap();
    }
    model::scan(root, &root.join(".claude"), &[]);
    for (query, file, name) in [
        ("registrar duração requisição", "src/duration.rs", "trace_event"),
        ("cabeçalho requisição", "src/headers.py", "accept_message"),
        ("erro resposta", "src/response.ts", "rejectMessage"),
    ] {
        let report = knowledge::query(root, query, None, 8, 0, false).unwrap().0;
        let card = report["cards"].as_array().unwrap().iter().find(|c| c["name"] == name && c["source"]["file"] == file).unwrap_or_else(|| panic!("{report}"));
        if name == "rejectMessage" {
            assert!(card["matched_evidence"].is_null(), "Visible signatures should not be repeated");
        } else {
            assert!(!card["matched_evidence"].is_null());
        }
        let witnesses = &card["matched_evidence"];
        assert!(witnesses["identifiers"].as_array().map_or(0, Vec::len) + witnesses["literals"].as_array().map_or(0, Vec::len) <= 3);
        assert!(card.get("identifiers").is_none());
        assert_eq!(report["remote_model_calls"], 0);
        assert_eq!(report["local_model_calls"], 0);
    }
    std::fs::write(root.join("src/duration.rs"), "pub fn trace_event(request: &str) {}\n").unwrap();
    assert!(
        knowledge::query(root, "registrar duração requisição", None, 8, 0, false).unwrap().0["cards"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["source"]["file"] != "src/duration.rs")
    );
}

#[test]
fn file_scope_is_applied_before_broad_candidates_consume_the_reservoir() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/noise.rs"), (0..550).map(|n| format!("/// cobalt\npub fn noise{n}() {{}}\n")).collect::<String>()).unwrap();
    std::fs::write(root.join("src/target.rs"), "/// cobalt\npub fn actual_worker() {}\n").unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let report = knowledge::query(root, "cobalt absentQuartz", Some("src/target.rs"), 8, 0, false).unwrap().0;
    assert_eq!(report["cards"][0]["name"], "actual_worker", "{report}");
    assert_eq!(report["catalog"]["hydrated_candidates"], 1);
}

#[test]
fn literal_tail_is_searchable_and_initial_response_stays_compact() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("src")).unwrap();
    // Even the decision to keep a text must inspect its tail, rather than
    // rejecting a numeric preview before reaching the searchable word.
    let literal = format!("{}finalQuartzProof", "12345 ".repeat(120));
    let mut source = String::from("pub fn process_batch() {\n");
    for n in 0..16 {
        source.push_str(&format!("let _value{n} = \"ordinary{n}\";\n"));
    }
    source.push_str(&format!("let _last = \"{literal}\";\n}}\n"));
    std::fs::write(root.join("src/work.rs"), source).unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let report = knowledge::query(root, "finalQuartzProof", None, 8, 0, false).unwrap().0;
    let card = &report["cards"][0];
    assert_eq!(card["name"], "process_batch", "{report}");
    assert_eq!(card["detail_counts"]["literals"], 17);
    assert!(card["matched_evidence"]["literals"].as_array().unwrap().iter().any(|v| v["value"].as_str().unwrap_or_default().contains("finalQuartzProof")));
    assert!(card["literals"].is_null());
    let expanded = knowledge::query(root, "finalQuartzProof", None, 8, 0, true).unwrap().0;
    assert!(expanded["cards"][0]["matched_evidence"].is_null(), "Full evidence must not be duplicated");
    assert_eq!(expanded["cards"][0]["literals"][16]["value"], literal);
    let db = rusqlite::Connection::open(store::model_path(root)).unwrap();
    let raw: String = db.query_row("SELECT analysis FROM texts WHERE path='src/work.rs'", [], |r| r.get(0)).unwrap();
    let raw: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(raw["knowledge"]["cards"][0]["literals"][16]["value"], literal);
    assert_eq!(knowledge::audit::run(root).unwrap()["ok"], json!(true));
}
