#[path = "support/model.rs"]
mod model;

use mustard_core::io::knowledge::{self, Query};
use serde_json::{Value, json};
use std::path::Path;

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::create_dir_all(dir.path().join("cfg")).unwrap();
    std::fs::write(dir.path().join("mustard.json"), r#"{"language":{"text":"pt-BR","code":"en-US"}}"#).unwrap();
    std::fs::write(
        dir.path().join("docs/rules.md"),
        "# Inventory\nUnrelated document introduction.\n## Recuperar pedido\nPreserva tarefas concluídas antes de recuperar o pedido.\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("cfg/session.yaml"), "# Configure session origin validation\nsession:\n  originValidation: true\n  timeout: 300\n").unwrap();
    dir
}

fn options<'a>(text: &'a str, file: Option<&'a str>, detail: bool) -> Query<'a> {
    Query { text, file, limit: 8, depth: 0, all: false, detail, symbol: None, direction: knowledge::Direction::Outgoing, refresh: false }
}

fn query(root: &Path, text: &str) -> Value {
    knowledge::query(root, text, None, 8, 0, false).unwrap().0
}

fn git(root: &Path, args: &[&str]) {
    let run = mustard_core::platform::git::run(root, args);
    assert!(run.ok, "{}", run.stderr);
}

#[test]
fn resources_are_searchable_with_source_ranges_and_no_invented_symbols_or_semantics() {
    let dir = fixture();
    let root = dir.path();
    let (map, scan) = model::scan(root, &root.join(".claude"), &[]);
    assert_eq!(scan["resources"], 2);
    assert!(map["modules"].as_array().unwrap().is_empty());
    let report = query(root, "recuperar pedido");
    let item = &report["resources"][0];
    assert_eq!(item["source"]["file"], "docs/rules.md", "{report}");
    assert_eq!(item["source"]["line"], 3);
    assert_eq!(item["source"]["end_line"], 4);
    assert_eq!(item["semantic_proof"], false);
    assert_eq!(item["source"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(report["cards"], json!([]));
    assert_eq!(report["remote_model_calls"], 0);
    assert_eq!(report["local_model_calls"], 0);
    let config = query(root, "session origin validation");
    assert_eq!(config["resources"][0]["source"]["file"], "cfg/session.yaml", "{config}");
    assert_eq!(config["resources"][0]["kind"], "configuration");
    assert_eq!(query(root, "cfg/session.yaml")["resources"][0]["source"]["file"], "cfg/session.yaml");
    let missing = query(root, "UnknownQuantumIdentifier");
    assert_eq!(missing["resources"], json!([]));
    let (report, map) = knowledge::query_with(root, root, &options("recuperar pedido", None, true)).unwrap();
    let md = mustard_core::domain::knowledge::markdown(&report, &map);
    assert!(md.contains("Preserva tarefas concluídas"));
    assert!(md.contains("SHA-256"));
    let prepared = knowledge::for_source(root, root, "cfg/session.yaml", "");
    assert_eq!(prepared["resources"][0]["source"]["file"], "cfg/session.yaml");
    assert!(prepared["evidence_version"].as_str().is_some());
    let all = mustard_core::io::project_map::read(root).unwrap();
    let expected = serde_json::to_value(&all.resources).unwrap();
    mustard_core::io::project_map::write(root, &all).unwrap();
    assert_eq!(serde_json::to_value(mustard_core::io::project_map::read(root).unwrap().resources).unwrap(), expected);
    model::scan(root, &root.join(".claude"), &[]);
    assert_eq!(query(root, "recuperar pedido")["resources"], report["resources"]);
}

#[test]
fn changed_deleted_and_divergent_worktree_resources_never_supply_old_evidence() {
    let dir = fixture();
    let root = dir.path();
    model::scan(root, &root.join(".claude"), &[]);
    let copy = fixture();
    std::fs::write(copy.path().join("docs/rules.md"), "# Changed rule\nPedido não recuperável.\n").unwrap();
    let divergent = knowledge::query_with(root, copy.path(), &options("recuperar pedido", None, false)).unwrap().0;
    assert_eq!(divergent["resources"], json!([]));
    assert!(divergent["resource_coverage"]["stale_excerpts"].as_u64().unwrap() > 0);
    std::fs::write(root.join("docs/rules.md"), "# Changed rule\nPedido não recuperável.\n").unwrap();
    assert_eq!(query(root, "recuperar pedido")["resources"], json!([]));
    std::fs::remove_file(root.join("cfg/session.yaml")).unwrap();
    assert_eq!(query(root, "session origin validation")["resources"], json!([]));
    assert!(knowledge::for_source(root, root, "cfg/session.yaml", "").is_null());
}

#[test]
fn incremental_scan_updates_resources_and_removes_deleted_rows() {
    let dir = fixture();
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["add", "."]);
    git(root, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "seed"]);
    model::scan(root, &root.join(".claude"), &[]);
    let (_, unchanged) = model::scan(root, &root.join(".claude"), &[]);
    assert_eq!(unchanged["read"], json!([]), "{unchanged}");
    assert_eq!(unchanged["resources"], 2);
    std::fs::write(root.join("docs/rules.md"), "# Archive approved orders\nArchive only approved orders.\n").unwrap();
    let (_, changed) = model::scan(root, &root.join(".claude"), &[]);
    assert_eq!(changed["full"], false, "{changed}");
    assert_eq!(changed["read"], json!(["docs/rules.md"]), "{changed}");
    assert_eq!(query(root, "archive approved orders")["resources"][0]["source"]["file"], "docs/rules.md");
    std::fs::remove_file(root.join("cfg/session.yaml")).unwrap();
    std::fs::write(root.join("docs/new.md"), "# Dispatch invoice\nInvoice dispatch rules.\n").unwrap();
    let (map, changed) = model::scan(root, &root.join(".claude"), &[]);
    assert_eq!(changed["full"], false, "{changed}");
    assert_eq!(changed["read"], json!(["docs/new.md"]), "{changed}");
    assert!(!map["resources"].as_array().unwrap().iter().any(|file| file["path"] == "cfg/session.yaml"));
    assert_eq!(query(root, "session origin validation")["resources"], json!([]));
    assert_eq!(query(root, "invoice dispatch")["resources"][0]["source"]["file"], "docs/new.md");
}

#[test]
fn unsupported_oversized_binary_ignored_and_sensitive_files_have_explicit_coverage() {
    let dir = fixture();
    let root = dir.path();
    for file in [".env.json", "credentials.json", "package-lock.json"] {
        std::fs::write(root.join(file), "{\"doNotIngest\":true}").unwrap();
    }
    std::fs::write(root.join("large.json"), " ".repeat(262145)).unwrap();
    std::fs::write(root.join("binary.json"), b"foo\0bar").unwrap();
    std::fs::write(root.join("invalid.txt"), [0xff]).unwrap();
    std::fs::write(root.join("normal-config.json"), r#"{"type":"service_account","private_key":"fixture-never-a-real-key"}"#).unwrap();
    let (map, scan) = model::scan(root, &root.join(".claude"), &[]);
    for file in [".env.json", "credentials.json", "package-lock.json"] {
        assert!(!map["resources"].as_array().unwrap().iter().any(|entry| entry["path"] == file), "{file}");
    }
    assert_eq!(
        scan["resource_issues"],
        json!([
            {"file":"binary.json","reason":"binary"},{"file":"invalid.txt","reason":"non-utf8"},{"file":"large.json","reason":"too-large"}
            ,{"file":"normal-config.json","reason":"possible-sensitive-content"}
        ])
    );
    assert_eq!(query(root, "doNotIngest")["resources"], json!([]));
    assert!(!model::read_bytes(&root.join(".claude")).windows(b"fixture-never-a-real-key".len()).any(|bytes| bytes == b"fixture-never-a-real-key"));
}

#[test]
fn resource_detail_expands_the_same_evidence_and_edit_beyond_preview_invalidates_prepared_context() {
    let dir = fixture();
    let root = dir.path();
    let file = "cfg/options.json";
    let text = format!("{{\"filler\":\"{}\",\"approvalRevisionKey\":true,\"tail\":\"{}\"}}\n", "á".repeat(1200), "文".repeat(900));
    std::fs::write(root.join(file), &text).unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let (brief, _) = knowledge::query_with(root, root, &options("approvalRevisionKey", Some(file), false)).unwrap();
    let (full, _) = knowledge::query_with(root, root, &options("approvalRevisionKey", Some(file), true)).unwrap();
    assert!(brief["resources"][0]["text"].as_str().unwrap().contains("approvalRevisionKey"), "{brief}");
    assert_eq!(brief["resources"][0]["text_compacted"], true);
    assert_eq!(full["resources"][0]["text"], text);
    assert_eq!(brief["resources"][0]["source"], full["resources"][0]["source"]);
    assert_eq!(brief["resources"][0]["id"], full["resources"][0]["id"]);
    let before = knowledge::for_source(root, root, file, "approvalRevisionKey");
    assert!(before["evidence_version"].as_str().is_some());
    std::fs::write(root.join(file), text.replace(&"文".repeat(900), &format!("{}終","文".repeat(899)))).unwrap();
    assert!(knowledge::for_source(root, root, file, "approvalRevisionKey").is_null());
    model::scan(root, &root.join(".claude"), &[]);
    let after = knowledge::for_source(root, root, file, "approvalRevisionKey");
    assert_ne!(before["evidence_version"], after["evidence_version"]);
    assert_eq!(before["resources"][0]["text"], after["resources"][0]["text"]);
}

#[test]
fn generic_word_overlap_does_not_add_unrelated_documentation_to_the_context() {
    let dir = fixture();
    let root = dir.path();
    std::fs::write(root.join("docs/noise.md"),"# Create schema\nCreate the schema. This is a generic document.\n").unwrap();
    std::fs::write(root.join("docs/accounts.md"),"# Create invoice account schema\nDeclared requirements for the invoice account schema.\n").unwrap();
    model::scan(root,&root.join(".claude"),&[]);
    let report = query(root,"create invoice account schema");
    assert_eq!(report["resources"].as_array().unwrap().len(),1,"{report}");
    assert_eq!(report["resources"][0]["source"]["file"],"docs/accounts.md");
}
