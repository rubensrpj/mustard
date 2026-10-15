#[path = "support/model.rs"]
mod model;
use mustard_core::io::{knowledge, map_notes, project_map as store};
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

/// A gravação que troca só um arquivo TypeScript refaz as citações só do
/// documento que cita esse arquivo, e deixa para depois o perfil do projeto:
/// quem o lê nesse meio tempo o vê já com o arquivo novo, e a passada
/// seguinte sem mudança o grava.
#[test]
fn a_save_of_one_file_settles_only_the_documents_citing_it_and_leaves_the_profile_for_later() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("mustard.json"), "{}").unwrap();
    std::fs::write(root.join("src/store.ts"), "export function restoreLedger(): number { return 1; }\n").unwrap();
    std::fs::write(root.join("src/books.ts"), "export function closeBooks(): number { return 2; }\n").unwrap();
    std::fs::write(root.join("docs/recovery.md"), "# Recovery\nThe approved closing uses `restoreLedger`.\n").unwrap();
    std::fs::write(root.join("docs/closing.md"), "# Closing\nThe month ends with `closeBooks`.\n").unwrap();
    model::scan(root, &root.join(".claude"), &["--native"]);
    let db = || rusqlite::Connection::open(store::model_path(root)).unwrap();
    let cited_at = |resource: &str| db().query_row("SELECT ref_line FROM knowledge_resource_refs WHERE resource=?1", [resource], |r| r.get::<_, i64>(0)).unwrap();
    let stored_lines = || serde_json::from_str::<Value>(&db().query_row("SELECT payload FROM project_profile", [], |r| r.get::<_, String>(0)).unwrap()).unwrap()["size"]["lines"].clone();
    let mark = || db().query_row("SELECT mark FROM blocks WHERE name='project_profile'", [], |r| r.get::<_, Option<String>>(0)).unwrap();
    // A citação que a gravação refizesse voltaria à linha 2.
    db().execute("UPDATE knowledge_resource_refs SET ref_line=99 WHERE resource='docs/closing.md'", []).unwrap();
    let before = stored_lines();

    std::fs::write(root.join("src/store.ts"), "export function restoreLedger(): number {\n  return 3;\n}\n").unwrap();
    model::scan(root, &root.join(".claude"), &["--native"]);
    assert_eq!((cited_at("docs/recovery.md"), cited_at("docs/closing.md")), (2, 99));
    assert_eq!(knowledge::audit::run(root).unwrap()["issues"], json!([]), "the citing document follows the new symbol");
    assert_eq!((mark().as_deref(), stored_lines()), (Some("behind"), before.clone()));
    let read = mustard_core::io::project_profile::snapshot(root);
    assert_eq!(read["data"]["size"]["lines"], json!(before.as_u64().unwrap() + 2), "{read}");

    model::scan(root, &root.join(".claude"), &["--native"]);
    assert_eq!((mark().as_deref(), stored_lines()), (Some(""), read["data"]["size"]["lines"].clone()));
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
        &knowledge::Interpretation { rule_context: None,
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

/// A auditoria conta as frases de uma linha num projeto em TypeScript lido
/// pelo scan: a frase da função cujo arquivo não mudou e a do arquivo inteiro
/// seguem em dia; a do método cujo arquivo mudou e a da função cujo texto
/// analisado não é JSON envelhecem, sem derrubar a auditoria, que aponta o
/// texto estragado. O método e essa função passam a contar como funções sem
/// frase; a classe e a frase do arquivo inteiro não contam.
#[test]
fn audit_counts_fresh_and_stale_notes_and_functions_without_a_fresh_note() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR","code":"en-US"}}"#).unwrap();
    std::fs::write(root.join("src/price.ts"), "export function priceOf(amount: number): number { return amount * 2; }\n").unwrap();
    std::fs::write(root.join("src/order.ts"), "export class Order {\n  total(): number { return 1; }\n}\n").unwrap();
    std::fs::write(root.join("src/tax.ts"), "export function taxOf(amount: number): number { return amount / 10; }\n").unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let model = store::model_path(root);
    for (file, name, text) in [
        ("src/price.ts", "priceOf", "Calcula o preço cobrado do cliente."),
        ("src/price.ts", "", "Regras de preço da loja."),
        ("src/order.ts", "total", "Soma o pedido do cliente."),
        ("src/tax.ts", "taxOf", "Calcula o imposto do pedido."),
    ] {
        map_notes::write(&model, root, &map_notes::NewNote { file, name, text, spec: "loja" }).unwrap();
    }
    std::fs::write(root.join("src/order.ts"), "export class Order {\n  total(): number { return 2; }\n}\n").unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let db = rusqlite::Connection::open(&model).unwrap();
    db.execute("UPDATE texts SET analysis='not-json' WHERE path='src/tax.ts'", []).unwrap();
    drop(db);
    let audit = knowledge::audit::run(root).unwrap();
    let counts = &audit["counts"];
    assert_eq!((&counts["notes_fresh"], &counts["notes_stale"], &counts["functions_without_note"]), (&json!(2), &json!(2), &json!(2)), "{audit}");
    assert!(audit["issues"].as_array().unwrap().iter().any(|i| i["check"] == "invalid_analysis_json"), "{audit}");
}

/// Num arquivo em TypeScript lido pelo scan, com duas funções e a frase de
/// cada uma, mudar o corpo da segunda envelhece só a frase dela: a da
/// primeira segue em dia, e a do arquivo inteiro envelhece com ele.
#[test]
fn a_changed_function_ages_only_its_own_note_in_a_scanned_typescript_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR","code":"en-US"}}"#).unwrap();
    let body = |refund: &str| format!("export function charge(amount: number): number {{\n  return amount;\n}}\n\nexport function refund(amount: number): number {{\n  return {refund};\n}}\n");
    std::fs::write(root.join("src/pay.ts"), body("amount")).unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let model = store::model_path(root);
    for (name, text) in [("charge", "Cobra o cliente."), ("refund", "Devolve o dinheiro do cliente."), ("", "Pagamentos da loja.")] {
        map_notes::write(&model, root, &map_notes::NewNote { file: "src/pay.ts", name, text, spec: "loja" }).unwrap();
    }
    std::fs::write(root.join("src/pay.ts"), body("-amount")).unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let stale = |name: &str| map_notes::of_at(&model, "src/pay.ts", name).unwrap().unwrap().stale;
    assert_eq!((stale("charge"), stale("refund"), stale("")), (false, true, true));
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
