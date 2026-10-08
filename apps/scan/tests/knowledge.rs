#[path = "support/model.rs"]
mod model;

use mustard_core::io::knowledge::{self, Interpretation};
use std::path::Path;

fn options() -> knowledge::Query<'static> {
    knowledge::Query {
        text: "",
        file: None,
        limit: 8,
        depth: 2,
        all: false,
        detail: false,
        symbol: None,
        direction: knowledge::Direction::Outgoing,
        refresh: false,
    }
}

#[test]
fn attached_annotations_are_grounded_across_grammars_and_survive_incremental_scan() {
    let dir = seed();
    let root = dir.path();
    std::fs::write(root.join("src/lib.rs"),"mod store;\n/// @intent Recuperar\n/// o plano\n/// @domainRule Exigir aprovação\n/// @unknown Não virar regra\n/// texto desconhecido\npub fn restore() { let _s = \"@intent texto literal falso\"; store::write(); }\n").unwrap();
    std::fs::write(root.join("src/invoices.ts"),"/**\n * @intent Liquidar parcelas\n * @requires Fatura aprovada\n * @sideEffect Persiste baixa\n */\nexport function settleInvoice() {}\n").unwrap();
    std::fs::write(root.join("src/audit.py"),"# Documentation above must not hide the attached inner tags.\ndef expire():\n    \"\"\"\n    @intent Eliminar registros vencidos\n    @ensures Preserva registros recentes\n    \"\"\"\n    return None\n").unwrap();
    // A real Git checkout lets the next scan reuse unchanged declarations.
    let git = |args: &[&str]| mustard_core::platform::git::run(root, args);
    assert!(git(&["init", "-q"]).ok);
    assert!(git(&["add", "."]).ok);
    assert!(
        git(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "seed"
        ])
        .ok
    );
    let (first, _) = model::scan(root, &root.join(".claude"), &[]);
    let restored = knowledge::query(root, "restore", Some("src/lib.rs"), 8, 0, false)
        .unwrap()
        .0;
    let annotations = &restored["cards"][0]["annotations"];
    assert_eq!(annotations[0]["tag"], "intent");
    assert_eq!(annotations[0]["text"], "Recuperar o plano");
    assert_eq!(annotations[0]["line"], 2);
    assert_eq!(annotations[0]["end_line"], 3);
    assert_eq!(annotations[1]["tag"], "domainRule");
    assert_eq!(annotations.as_array().unwrap().len(), 2);
    assert!(
        restored["cards"][0]["annotation_status"]
            .as_str()
            .unwrap()
            .contains("not semantic proof")
    );
    for (query, file, tag, line) in [
        ("settleInvoice", "src/invoices.ts", "requires", 3),
        ("expire", "src/audit.py", "ensures", 5),
    ] {
        let found = knowledge::query(root, query, Some(file), 8, 0, false)
            .unwrap()
            .0;
        assert_eq!(found["cards"][0]["annotations"][1]["tag"], tag, "{found}");
        assert_eq!(found["cards"][0]["annotations"][1]["line"], line, "{found}");
    }
    let prepared = knowledge::for_source(root, root, "src/audit.py", "expire");
    assert_eq!(
        prepared["annotations"][0]["annotations"][0]["tag"],
        "intent"
    );
    assert_eq!(prepared["semantic_proof"], false);
    let version = prepared["evidence_version"].clone();
    std::fs::write(
        root.join("src/store.rs"),
        "pub fn write() { }\n// changed\n",
    )
    .unwrap();
    let (second, report) = model::scan(root, &root.join(".claude"), &[]);
    assert_eq!(report["full"], false, "{report}");
    assert_eq!(
        report["read"],
        serde_json::json!(["src/store.rs"]),
        "{report}"
    );
    let persisted = |map: &serde_json::Value| {
        map["modules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|module| module["path"] == "src/audit.py")
            .unwrap()["declarations"][0]["annotations"]
            .clone()
    };
    assert_eq!(persisted(&first), persisted(&second));
    assert_eq!(
        knowledge::for_source(root, root, "src/audit.py", "expire")["evidence_version"],
        version
    );
    std::fs::write(root.join("src/audit.py"),"def expire():\n    \"\"\"\n    @intent Arquivar registros vencidos\n    \"\"\"\n    return None\n").unwrap();
    assert!(knowledge::for_source(root, root, "src/audit.py", "expire").is_null());
    model::scan(root, &root.join(".claude"), &[]);
    assert_ne!(
        knowledge::for_source(root, root, "src/audit.py", "expire")["evidence_version"],
        version
    );
}

#[test]
fn exact_symbol_navigation_recovers_consumers_and_refuses_stale_or_unknown_identities() {
    let dir = seed();
    let root = dir.path();
    std::fs::write(root.join("src/lib.rs"),"mod store;\n/// Restaura o plano.\npub fn restore() { store::write(); }\npub fn entry() { restore(); }\npub fn unrelated() {}\n").unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let found = knowledge::query(root, "write", Some("src/store.rs"), 8, 0, false)
        .unwrap()
        .0;
    let id = found["cards"][0]["id"].as_str().unwrap();
    let opts = knowledge::Query {
        symbol: Some(id),
        direction: knowledge::Direction::Callers,
        ..options()
    };
    let result = knowledge::query_with(root, root, &opts).unwrap().0;
    let names: Vec<_> = result["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|card| card["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["write", "restore", "entry"], "{result}");
    assert_eq!(result["cards"][0]["retrieval"], "exact-symbol");
    assert_eq!(result["navigation"]["paths"][1]["distance"], 2);
    assert_eq!(result["navigation"]["paths"][1]["via"], "caller");
    let limited = knowledge::query_with(root, root, &knowledge::Query { depth: 1, ..opts })
        .unwrap()
        .0;
    assert_eq!(limited["cards"].as_array().unwrap().len(), 2);
    assert_eq!(limited["navigation"]["omitted_destinations"], 1);
    let out = knowledge::query_with(
        root,
        root,
        &knowledge::Query {
            direction: knowledge::Direction::Outgoing,
            ..opts
        },
    )
    .unwrap()
    .0;
    assert_eq!(out["cards"].as_array().unwrap().len(), 1);
    let copy = seed();
    let divergent = knowledge::query_with(root, copy.path(), &opts).unwrap().0;
    assert_eq!(divergent["cards"].as_array().unwrap().len(), 1);
    assert_eq!(divergent["navigation"]["stale_destinations"], 1);
    let absent = knowledge::query_with(
        root,
        root,
        &knowledge::Query {
            symbol: Some("src/missing.rs:1:write"),
            ..options()
        },
    )
    .unwrap()
    .0;
    assert_eq!(absent["cards"], serde_json::json!([]));
    assert!(
        knowledge::query_with(
            root,
            root,
            &knowledge::Query {
                text: "write",
                ..opts
            }
        )
        .is_err()
    );
    std::fs::write(root.join("src/store.rs"), "pub fn write() { panic!(); }\n").unwrap();
    let stale = knowledge::query_with(root, root, &opts).unwrap().0;
    assert_eq!(stale["cards"], serde_json::json!([]));
}

#[test]
fn refresh_queue_is_scoped_and_keeps_old_claims_out_of_current_evidence() {
    let dir = seed();
    let root = dir.path();
    model::scan(root, &root.join(".claude"), &[]);
    let sources = knowledge::query(root, "backup", None, 8, 2, false)
        .unwrap()
        .0["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|card| serde_json::from_value(card["source"].clone()).unwrap())
        .collect();
    let note = Interpretation {
        id: "recovery".into(),
        title: "Recuperação de desastre".into(),
        text: "O plano é recuperado e persiste.".into(),
        status: "reviewed".into(),
        origin: "fixture".into(),
        sources,
    };
    knowledge::record(root, &note).unwrap();
    let opts = knowledge::Query {
        refresh: true,
        ..options()
    };
    let current = knowledge::query_with(root, root, &opts).unwrap().0;
    assert_eq!(current["matching_stale_count"], 0);
    std::fs::write(
        root.join("src/store.rs"),
        "// declaration moved\npub fn write() { panic!(); }\n",
    )
    .unwrap();
    let (result, map) = knowledge::query_with(root, root, &opts).unwrap();
    assert_eq!(result["remote_model_calls"], 0);
    assert_eq!(result["matching_stale_count"], 1);
    assert_eq!(result["cards"], serde_json::json!([]));
    assert_eq!(result["interpretations"], serde_json::json!([]));
    let candidate = &result["refresh_candidates"][0];
    assert_eq!(candidate["changed_sources"][0]["reason"], "content-changed");
    assert_eq!(
        candidate["changed_sources"][0]["previous_source"]["file"],
        "src/store.rs"
    );
    assert_eq!(candidate["unchanged_sources"].as_array().unwrap().len(), 1);
    assert!(candidate.get("previous_text").is_none());
    assert_ne!(
        candidate["changed_sources"][0]["current_file"]["sha256"],
        candidate["changed_sources"][0]["previous_source"]["sha256"]
    );
    assert!(mustard_core::domain::knowledge::markdown(&result, &map).contains("content-changed"));
    let full = knowledge::query_with(
        root,
        root,
        &knowledge::Query {
            detail: true,
            ..opts
        },
    )
    .unwrap()
    .0;
    assert_eq!(full["refresh_candidates"][0]["previous_text"], note.text);
    let unrelated = knowledge::query_with(
        root,
        root,
        &knowledge::Query {
            file: Some("src/unrelated.rs"),
            ..opts
        },
    )
    .unwrap()
    .0;
    assert_eq!(unrelated["matching_stale_count"], 0);
    std::fs::remove_file(root.join("src/store.rs")).unwrap();
    let missing = knowledge::query_with(root, root, &opts).unwrap().0;
    assert_eq!(
        missing["refresh_candidates"][0]["changed_sources"][0]["reason"],
        "missing-unreadable-or-outside-tree"
    );
    assert_eq!(
        knowledge::interpretations(root).unwrap()[0].sources,
        note.sources
    );
    assert!(knowledge::record(root, &note).is_err());
}

fn seed() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "mod store;\n/// Restaura o backup do plano.\npub fn restore() { store::write(); }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/store.rs"),
        "/// Persiste o plano restaurado.\npub fn write() {}\n",
    )
    .unwrap();
    dir
}

#[test]
fn scan_persists_grounded_flows_and_renders_a_report_without_a_model() {
    let dir = seed();
    model::scan(dir.path(), &dir.path().join(".claude"), &[]);
    let (report, map) =
        knowledge::query(dir.path(), "restaurar backup", None, 8, 2, false).unwrap();
    let cards = report["cards"].as_array().unwrap();
    assert!(cards.iter().any(|card| card["name"] == "restore"));
    assert!(cards.iter().any(|card| card["name"] == "write"), "{report}");
    assert_eq!(report["remote_model_calls"], 0);
    assert!(
        cards
            .iter()
            .all(|card| card["source"]["sha256"].as_str().unwrap().len() == 64)
    );
    let text = mustard_core::domain::knowledge::markdown(&report, &map);
    assert!(text.contains("backup"));
    assert!(text.contains("src/store.rs"));
    assert!(text.contains("unique-static-target"));
    std::fs::write(dir.path().join("src/store.rs"), "pub fn renamed() {}\n").unwrap();
    let (changed, _) = knowledge::query(dir.path(), "restaurar backup", None, 8, 2, false).unwrap();
    assert!(
        !changed["cards"]
            .as_array()
            .unwrap()
            .iter()
            .any(|card| card["name"] == "write")
    );
    assert!(
        changed["cards"][0]["outgoing"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{changed}"
    );
}

#[test]
fn interpretations_survive_rescans_but_never_survive_a_source_change_as_current() {
    let dir = seed();
    let root = dir.path();
    model::scan(root, &root.join(".claude"), &[]);
    let (report, _) = knowledge::query(root, "restaurar backup", None, 8, 2, false).unwrap();
    let sources = report["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|card| serde_json::from_value(card["source"].clone()).unwrap())
        .collect();
    let note = Interpretation {
        id: "restore-plan".into(),
        title: "Recuperação de desastre".into(),
        text: "O backup restaura o plano e o persiste.".into(),
        status: "reviewed".into(),
        origin: "integration-fixture".into(),
        sources,
    };
    knowledge::record(root, &note).unwrap();
    model::scan(root, &root.join(".claude"), &["--all"]);
    let result = knowledge::query(root, "recuperação desastre", None, 8, 2, false)
        .unwrap()
        .0;
    assert_eq!(result["interpretations"][0]["id"], "restore-plan");
    std::fs::write(
        root.join("src/store.rs"),
        "pub fn write() { panic!(\"not persisted\"); }\n",
    )
    .unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let result = knowledge::query(root, "recuperação desastre", None, 8, 2, false)
        .unwrap()
        .0;
    assert_eq!(result["interpretations"], serde_json::json!([]));
    assert_eq!(
        result["stale_interpretations"],
        serde_json::json!(["restore-plan"])
    );
}

#[test]
fn querying_a_different_wave_tree_does_not_trust_the_main_checkouts_hash() {
    let main = seed();
    let copy = tempfile::tempdir().unwrap();
    model::scan(main.path(), &main.path().join(".claude"), &[]);
    std::fs::create_dir(copy.path().join("src")).unwrap();
    for file in ["lib.rs", "store.rs"] {
        std::fs::copy(
            main.path().join("src").join(file),
            copy.path().join("src").join(file),
        )
        .unwrap();
    }
    std::fs::write(copy.path().join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
    let result = knowledge::query_at(main.path(), copy.path(), "backup", None, 8, 2, false)
        .unwrap()
        .0;
    assert!(
        !result["cards"]
            .as_array()
            .unwrap()
            .iter()
            .any(|card| card["name"] == "restore")
    );
    assert!(
        result["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap.as_str().unwrap().contains("refresh"))
    );
    assert_eq!(
        knowledge::for_source(main.path(), copy.path(), "src/lib.rs", "restore"),
        serde_json::Value::Null
    );
    assert!(Path::new(main.path()).join("src/lib.rs").exists());
}

#[test]
fn changing_a_caller_removes_its_link_from_the_unchanged_target() {
    let dir = seed();
    model::scan(dir.path(), &dir.path().join(".claude"), &[]);
    let before = knowledge::query(dir.path(), "write", Some("src/store.rs"), 8, 0, false)
        .unwrap()
        .0;
    assert!(!before["cards"][0]["callers"].as_array().unwrap().is_empty());
    std::fs::write(dir.path().join("src/lib.rs"), "pub fn restore() {}\n").unwrap();
    let after = knowledge::query(dir.path(), "write", Some("src/store.rs"), 8, 0, false)
        .unwrap()
        .0;
    assert!(after["cards"][0]["callers"].as_array().unwrap().is_empty());
    assert!(
        after["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap.as_str().unwrap().contains("static relations excluded"))
    );
}

#[test]
fn prepared_knowledge_checks_every_interpretation_source_in_the_wave() {
    let main = seed();
    let copy = seed();
    let root = main.path();
    model::scan(root, &root.join(".claude"), &[]);
    let sources = knowledge::query(root, "backup", None, 8, 2, false)
        .unwrap()
        .0["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|card| serde_json::from_value(card["source"].clone()).unwrap())
        .collect();
    let note = Interpretation {
        id: "prepared".into(),
        title: "Restore".into(),
        text: "Recover and persist the plan.".into(),
        status: "reviewed".into(),
        origin: "fixture".into(),
        sources,
    };
    knowledge::record(root, &note).unwrap();
    assert_eq!(
        knowledge::for_source(root, copy.path(), "src/lib.rs", "restore")["interpretations"][0]["id"],
        "prepared"
    );
    std::fs::write(
        copy.path().join("src/store.rs"),
        "pub fn write() { panic!(); }\n",
    )
    .unwrap();
    assert!(knowledge::for_source(root, copy.path(), "src/lib.rs", "restore").is_null());
    let bytes = std::fs::read(copy.path().join("src/store.rs")).unwrap();
    let mut hash = mustard_core::io::sha256::Sha256::new();
    hash.update(&bytes);
    let mut changed = note.clone();
    let source = changed
        .sources
        .iter_mut()
        .find(|source| source.file == "src/store.rs")
        .unwrap();
    source.sha256 = hash.hex_digest();
    source.line = 1;
    source.end_line = 1;
    changed.id = "wave-review".into();
    changed.text = "Persistence now panics.".into();
    assert!(knowledge::record(root, &changed).is_err());
    knowledge::record_at(root, copy.path(), &changed).unwrap();
    assert_eq!(
        knowledge::query_at(root, copy.path(), "Persistence", None, 8, 0, false)
            .unwrap()
            .0["interpretations"][0]["id"],
        "wave-review"
    );
    let main_answer = knowledge::query(root, "Persistence", None, 8, 0, false)
        .unwrap()
        .0;
    assert!(
        !main_answer["interpretations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note["id"] == "wave-review")
    );
    assert!(
        main_answer["stale_interpretations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| id == "wave-review")
    );
}

#[test]
fn discovery_covers_documented_intent_across_languages_and_preserves_identifiers() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(
        root.join("mustard.json"),
        r#"{"language":{"text":"pt-BR","code":"en-US"}}"#,
    )
    .unwrap();
    std::fs::write(root.join("src/payments.ts"),
        "/** Liquida as parcelas do pagamento. */\nexport function settleInvoice() { return 'paid'; }\n").unwrap();
    std::fs::write(
        root.join("src/recovery.py"),
        "def reconcile():\n    \"\"\"Restaura o backup da conta.\"\"\"\n    return True\n",
    )
    .unwrap();
    std::fs::write(root.join("src/retention.rs"),
        "//! Removes expired audit records.\n/// Delete expired audit records.\npub fn execute() {}\n").unwrap();
    std::fs::write(
        root.join("src/noise.rs"),
        (0..20)
            .map(|n| {
                format!(
                    "/// Exibe o pagamento e backup em diagnóstico.\npub fn inspect_{n}() {{}}\n"
                )
            })
            .collect::<String>(),
    )
    .unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    for (query, file, name) in [
        ("liquidar parcelas", "src/payments.ts", "settleInvoice"),
        ("restaurar backup", "src/recovery.py", "reconcile"),
        (
            "delete expired audit records",
            "src/retention.rs",
            "execute",
        ),
        ("settleInvoice", "src/payments.ts", "settleInvoice"),
    ] {
        let r = knowledge::query(root, query, None, 3, 2, false).unwrap().0;
        assert!(
            r["cards"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["source"]["file"] == file && c["name"] == name),
            "{query}: {r}"
        );
        assert_eq!(r["remote_model_calls"], 0);
        assert_eq!(r["local_hybrid_index"], true, "{r}");
        if query == name {
            assert_eq!(r["cards"][0]["name"], name);
        }
    }
    let absent = knowledge::query(root, "zzqvxxnonexistent", None, 3, 2, false)
        .unwrap()
        .0;
    assert_eq!(absent["cards"], serde_json::json!([]));
    assert!(
        absent["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g.as_str().unwrap().contains("absence is not proof"))
    );
}

#[test]
fn summary_expands_the_same_sources_without_losing_contracts_or_mutating_the_pack() {
    let dir = seed();
    let root = dir.path();
    let long = "/// Condição documentada na origem.\n".repeat(35);
    std::fs::write(
        root.join("src/lib.rs"),
        format!("mod store;\n{long}pub fn restore() {{ store::write(); }}\n"),
    )
    .unwrap();
    model::scan(root, &root.join(".claude"), &[]);
    let query = knowledge::Query {
        text: "restore",
        file: Some("src/lib.rs"),
        limit: 8,
        depth: 0,
        all: false,
        detail: false,
        symbol: None,
        direction: knowledge::Direction::Outgoing,
        refresh: false,
    };
    let summary = knowledge::query_with(root, root, &query).unwrap().0;
    let full = knowledge::query_with(
        root,
        root,
        &knowledge::Query {
            detail: true,
            ..query
        },
    )
    .unwrap()
    .0;
    assert_eq!(summary["projection"], "summary");
    assert_eq!(full["projection"], "detail");
    assert!(
        !summary["cards"].as_array().unwrap().is_empty(),
        "{summary}"
    );
    let a = &summary["cards"][0];
    let b = &full["cards"][0];
    assert_eq!(a["id"], b["id"]);
    assert_eq!(a["source"], b["source"]);
    assert_eq!(a["contracts"], b["contracts"]);
    assert_eq!(a["routes"], b["routes"]);
    assert_eq!(a["text_compacted"], true);
    assert!(
        b["documentation"].as_str().unwrap().len() > a["documentation"].as_str().unwrap().len()
    );
    assert_eq!(
        a["outgoing"][0]["resolution"],
        b["outgoing"][0]["resolution"]
    );
    assert!(b["outgoing"][0]["source"]["sha256"].is_string());
    assert!(summary.to_string().len() < full.to_string().len());
    assert_eq!(
        knowledge::query_with(
            root,
            root,
            &knowledge::Query {
                detail: true,
                ..query
            }
        )
        .unwrap()
        .0,
        full
    );
}

#[test]
fn prepared_evidence_keeps_a_compact_interpretation_and_tracks_hidden_edits() {
    let dir = seed();
    let root = dir.path();
    model::scan(root, &root.join(".claude"), &[]);
    let source = knowledge::query(root, "write", Some("src/store.rs"), 8, 0, false)
        .unwrap()
        .0["cards"][0]["source"]
        .clone();
    let mut note = Interpretation {
        id: "persistence".into(),
        title: "Persistência".into(),
        text: "Parte comum. ".repeat(90),
        status: "reviewed".into(),
        origin: "fixture".into(),
        sources: vec![serde_json::from_value(source).unwrap()],
    };
    knowledge::record(root, &note).unwrap();
    let before = knowledge::for_source(root, root, "src/store.rs", "write");
    assert!(before.to_string().len() < 3000);
    assert_eq!(before["interpretations"][0]["id"], "persistence");
    assert_eq!(before["interpretations"][0]["text_compacted"], true);
    note.text
        .push_str("Condição adicional fora do trecho inicial.");
    knowledge::record(root, &note).unwrap();
    let after = knowledge::for_source(root, root, "src/store.rs", "write");
    assert_eq!(
        before["interpretations"][0]["text"],
        after["interpretations"][0]["text"]
    );
    assert_ne!(before["evidence_version"], after["evidence_version"]);
}
