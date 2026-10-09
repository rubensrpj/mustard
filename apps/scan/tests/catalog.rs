#[path = "support/model.rs"]
mod model;
use mustard_core::io::{knowledge, project_map as store};
use serde_json::{Value, json};

fn query(root: &std::path::Path, text: &str) -> Value {
    knowledge::query(root, text, None, 8, 0, false).unwrap().0
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("mustard.json"), r#"{"language":{"text":"pt-BR","code":"en-US"}}"#).unwrap();
    std::fs::write(dir.path().join("src/store.rs"), "pub fn restoreLedger() {}\npub fn helper() {}\n").unwrap();
    std::fs::write(dir.path().join("docs/recovery.md"), "# Recuperação contábil\nO fechamento aprovado usa `restoreLedger`.\n").unwrap();
    dir
}

#[test]
fn executable_files_without_declarations_are_addressable_without_inventing_functions() {
    let dir = fixture();
    let root = dir.path();
    let text = "const { handler } = configure({\n  onValidateItem: (request) => request.headers['validation-key'],\n});\nexport { handler };\n";
    std::fs::write(root.join("src/guard.ts"), text).unwrap();
    // More broad, partial matches than the ordinary candidate reservoir.
    std::fs::write(root.join("src/noise.rs"), (0..540).map(|n| format!("/// Validate\npub fn noise{n}() {{}}\n")).collect::<String>()).unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let report = query(root, "onValidateItem");
    let card = &report["cards"][0];
    assert_eq!(card["source"]["file"], "src/guard.ts", "{report}");
    assert_eq!(card["kind"], "source-file");
    assert_eq!(card["name"], "guard.ts");
    assert_eq!(card["source"]["end_line"], 4);
    assert!(card["identifiers"].as_str().unwrap().contains("onValidateItem"));
    assert_eq!(knowledge::audit::run(root).unwrap()["ok"], true);
    std::fs::write(root.join("src/guard.ts"), "changed").unwrap();
    assert!(query(root, "onValidateItem")["cards"].as_array().unwrap().iter().all(|c| c["source"]["file"] != "src/guard.ts"));
}

#[test]
fn explicit_document_references_find_code_and_navigation_finds_documents_with_both_receipts_current() {
    let dir = fixture();
    let root = dir.path();
    model::scan(root, &root.join(".claude"), &[]);
    let report = query(root, "fechamento aprovado");
    let card = &report["cards"][0];
    assert_eq!(card["name"], "restoreLedger", "{report}");
    assert_eq!(card["retrieval"], "explicit-resource-reference");
    let opts = knowledge::Query {
        text: "",
        file: None,
        limit: 8,
        depth: 0,
        all: false,
        detail: false,
        symbol: Some(card["id"].as_str().unwrap()),
        direction: knowledge::Direction::Outgoing,
        refresh: false,
    };
    let nav = knowledge::query_with(root, root, &opts).unwrap().0;
    assert_eq!(nav["resources"][0]["source"]["file"], "docs/recovery.md");
    assert_eq!(nav["resources"][0]["reference"]["kind"], "explicit-unique-symbol");
    let prepared = knowledge::for_source(root, root, "src/store.rs", "restoreLedger");
    assert_eq!(prepared["documents"][0]["source"]["file"], "docs/recovery.md");
    std::fs::write(root.join("docs/recovery.md"), "# Changed\nUnrelated now.\n").unwrap();
    assert_eq!(query(root, "fechamento aprovado")["cards"], json!([]));
    assert_eq!(knowledge::query_with(root, root, &opts).unwrap().0["resources"], json!([]));
    assert!(knowledge::for_source(root, root, "src/store.rs", "restoreLedger").is_null());
    std::fs::write(root.join("docs/recovery.md"), "# Recuperação contábil\nO fechamento aprovado usa `restoreLedger`.\n").unwrap();
    std::fs::write(root.join("src/store.rs"), "pub fn restoreLedger() { panic!(); }\n").unwrap();
    assert_eq!(query(root, "fechamento aprovado")["cards"], json!([]));
}

#[test]
fn ambiguous_names_and_examples_do_not_create_document_links_and_line_addresses_select_the_narrowest_symbol() {
    let dir = fixture();
    let root = dir.path();
    std::fs::write(root.join("src/other.rs"), "pub fn restoreLedger() {}\n").unwrap();
    std::fs::write(root.join("docs/recovery.md"), "# Recuperação contábil\nO fechamento aprovado usa `restoreLedger`.\n```text\n`helper`\n```\n").unwrap();
    std::fs::write(root.join("docs/path.md"), "# Reconciliação financeira\nConfira [implementação](../src/store.rs#L2).\n").unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    assert_eq!(query(root, "fechamento aprovado")["cards"], json!([]));
    assert_eq!(query(root, "reconciliação financeira")["cards"][0]["name"], "helper");
    let db = rusqlite::Connection::open(store::model_path(root)).unwrap();
    assert_eq!(db.query_row("SELECT count(*) FROM knowledge_ref_issues WHERE reason='ambiguous-symbol'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(db.query_row("SELECT count(*) FROM knowledge_resource_refs WHERE resource='docs/recovery.md'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    drop(db);
    std::fs::remove_file(root.join("src/other.rs")).unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    assert_eq!(query(root, "fechamento aprovado")["cards"][0]["name"], "restoreLedger");
}

#[test]
fn exact_lookup_hydrates_only_relevant_packs_and_uses_path_indexes() {
    let dir = fixture();
    let root = dir.path();
    for n in 0..120 {
        std::fs::write(root.join(format!("src/other{n}.rs")), format!("pub fn unrelated{n}() {{}}\n")).unwrap();
    }
    model::scan(root, &root.join(".claude"), &[]);
    // An unrelated malformed pack must not be decoded during exact lookup.
    let db = rusqlite::Connection::open(store::model_path(root)).unwrap();
    db.execute("UPDATE texts SET analysis='not-json' WHERE path='src/other0.rs'", []).unwrap();
    let report = query(root, "restoreLedger");
    assert_eq!(report["cards"][0]["name"], "restoreLedger", "{report}");
    assert!(report["catalog"]["hydrated_candidates"].as_u64().unwrap() < 10, "{report}");
    assert!(report["catalog"]["symbols"].as_u64().unwrap() > 120);
    let plan = db.query_row("EXPLAIN QUERY PLAN SELECT analysis FROM texts WHERE path='src/store.rs'", [], |r| r.get::<_, String>(3)).unwrap();
    assert!(plan.contains("knowledge_texts_path"), "{plan}");
}

#[test]
fn resource_index_updates_preserve_untouched_postings_and_full_match_is_selected_before_candidate_cutoff() {
    let dir = fixture();
    let root = dir.path();
    for n in 0..280 {
        std::fs::write(root.join(format!("docs/noise{n}.md")), "# Session validation\nSession origin validation.\n").unwrap();
    }
    std::fs::write(
        root.join("docs/target.md"),
        format!("# Session validation\n{} approvalRevisionKey session origin validation\n", (0..150).map(|n| format!("word{n} ")).collect::<String>()),
    )
    .unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let db = rusqlite::Connection::open(store::model_path(root)).unwrap();
    let id: i64 = db.query_row("SELECT id FROM resource_positions WHERE path='docs/noise0.md'", [], |r| r.get(0)).unwrap();
    drop(db);
    let found = query(root, "session origin validation approvalRevisionKey");
    assert_eq!(found["resources"][0]["source"]["file"], "docs/target.md", "{found}");
    std::fs::write(root.join("docs/target.md"), "# Replacement\nreplacementUniqueToken\n").unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let db = rusqlite::Connection::open(store::model_path(root)).unwrap();
    assert_eq!(db.query_row("SELECT id FROM resource_positions WHERE path='docs/noise0.md'", [], |r| r.get::<_, i64>(0)).unwrap(), id);
    assert_eq!(query(root, "approvalRevisionKey")["resources"], json!([]));
    assert_eq!(query(root, "replacementUniqueToken")["resources"][0]["source"]["file"], "docs/target.md");
}

#[test]
fn derived_catalogue_rebuild_preserves_sources_and_reviewed_notes() {
    let dir = fixture();
    let root = dir.path();
    model::scan(root, &root.join(".claude"), &[]);
    let report = query(root, "restoreLedger");
    let source = serde_json::from_value(report["cards"][0]["source"].clone()).unwrap();
    knowledge::record(
        root,
        &knowledge::Interpretation {
            id: "receipt".into(),
            title: "Conferência".into(),
            text: "Checked evidence".into(),
            status: "reviewed".into(),
            origin: "test reviewer".into(),
            sources: vec![source],
        },
    )
    .unwrap();
    let db = rusqlite::Connection::open(store::model_path(root)).unwrap();
    let old: String = db.query_row("SELECT analysis FROM texts WHERE path='src/store.rs'", [], |r| r.get(0)).unwrap();
    db.execute("UPDATE blocks SET version=0 WHERE name='knowledge_index'", []).unwrap();
    drop(db);
    let after = query(root, "restoreLedger");
    assert_eq!(after["cards"][0]["id"], report["cards"][0]["id"]);
    assert_eq!(knowledge::interpretations(root).unwrap().len(), 1);
    let db = rusqlite::Connection::open(store::model_path(root)).unwrap();
    assert_eq!(db.query_row("SELECT analysis FROM texts WHERE path='src/store.rs'", [], |r| r.get::<_, String>(0)).unwrap(), old);
    assert_eq!(after["scan_snapshot"]["consistent_generation"], true);
}

#[test]
fn detailed_native_report_groups_only_current_static_evidence_and_exports_it() {
    let dir = fixture();
    let root = dir.path();
    std::fs::write(root.join("src/store.rs"), "pub fn restoreLedger() { helper(); }\npub fn helper() {}\n").unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let (report, map) = knowledge::query(root, "", None, 8, 2, true).unwrap();
    let groups = report["capability_candidates"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "{report}");
    assert_eq!(groups[0]["static_edges"].as_array().unwrap().len(), 1);
    assert_eq!(groups[0]["business_meaning"], "not inferred");
    let md = mustard_core::domain::knowledge::markdown(&report, &map);
    assert!(md.contains("structural group"));
    assert!(md.contains("restoreLedger"));
}

#[test]
fn native_audit_checks_receipts_indexes_and_plans_and_detects_a_missing_posting() {
    let dir = fixture();
    let root = dir.path();
    model::scan(root, &root.join(".claude"), &[]);
    let audit = knowledge::audit::run(root).unwrap();
    assert_eq!(audit["ok"], true, "{audit}");
    assert_eq!(audit["remote_model_calls"], 0);
    assert_eq!(audit["counts"]["document_references"], 1);
    let plans = audit["query_plans"].to_string();
    assert!(plans.contains("knowledge_by_name"));
    assert!(plans.contains("resource_source_path"));
    let db = rusqlite::Connection::open(store::model_path(root)).unwrap();
    db.execute("DELETE FROM knowledge_fts WHERE rowid=(SELECT rowid FROM knowledge_symbols LIMIT 1)", []).unwrap();
    drop(db);
    let broken = knowledge::audit::run(root).unwrap();
    assert_eq!(broken["ok"], false, "{broken}");
    assert!(broken["issues"].as_array().unwrap().iter().any(|i| i["check"] == "missing_symbol_posting"));
    let db = rusqlite::Connection::open(store::model_path(root)).unwrap();
    db.execute("DELETE FROM knowledge_fts WHERE rowid IN (SELECT rowid FROM knowledge_symbols WHERE name='helper')", []).unwrap();
    db.execute("DELETE FROM knowledge_symbols WHERE name='helper'", []).unwrap();
    drop(db);
    let missing = knowledge::audit::run(root).unwrap();
    assert!(missing["issues"].as_array().unwrap().iter().any(|i| i["check"] == "canonical_symbol_missing"), "{missing}");
}

#[test]
fn same_line_members_have_distinct_stable_identities_and_old_pack_versions_request_refresh() {
    let dir = fixture();
    let root = dir.path();
    std::fs::write(root.join("src/compare.ts"), "export const byName = (a: { name: string }, b: { name: string }) => a.name.localeCompare(b.name);\n").unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let report = query(root, "name");
    let cards = report["cards"].as_array().unwrap();
    let members: Vec<_> = cards.iter().filter(|c| c["source"]["file"] == "src/compare.ts" && c["name"] == "name").collect();
    assert_eq!(members.len(), 2, "{report}");
    assert_ne!(members[0]["id"], members[1]["id"]);
    assert_eq!(report["retrieval_method"], "exact-name-index");
    let ids: Vec<_> = members.iter().map(|c| c["id"].clone()).collect();
    model::scan(root, &root.join(".claude"), &[]);
    let after = query(root, "name");
    assert_eq!(after["cards"].as_array().unwrap().iter().map(|c| c["id"].clone()).collect::<Vec<_>>(), ids);
    let db = rusqlite::Connection::open(store::model_path(root)).unwrap();
    db.execute("UPDATE texts SET analysis=json_set(analysis,'$.knowledge.version',1)", []).unwrap();
    db.execute("UPDATE blocks SET version=0 WHERE name='knowledge_index'", []).unwrap();
    drop(db);
    let old = query(root, "name");
    assert_eq!(old["cards"], json!([]));
    assert!(old["catalog"]["outdated_packs"].as_u64().unwrap() > 0);
    assert!(old["gaps"].as_array().unwrap().iter().any(|g| g.as_str().unwrap().contains("earlier or missing version")));
    model::scan(root, &root.join(".claude"), &[]);
    assert_eq!(query(root, "name")["cards"].as_array().unwrap().len(), 2);
}
