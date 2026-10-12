use super::*;
use crate::io::fs;

fn fixture() -> (tempfile::TempDir, Profile) {
    let dir = tempfile::tempdir().unwrap();
    let text = "fn read() {}\n";
    fs::write_atomic(dir.path().join("store.rs"), text.as_bytes()).unwrap();
    project_map::write_text(dir.path(), r#"{"modules":[]}"#).unwrap();
    let profile = Profile { name: "database".into(), description: "Use for database tasks in this project; inspect current store examples.".into(),
        paths: vec!["store.rs".into()], procedure: "Reuse the existing transaction boundaries; confirm their behavior in code and tests.".into(),
        sources: vec![Source { file: "store.rs".into(), line: 1, end_line: 1, sha256: digest(text.as_bytes()) }], origin: "authored".into(), reviewed: Some("forged".into()) };
    (dir, profile)
}

#[test]
fn drafts_need_exact_review_and_changed_sources_withdraw_them() {
    let (dir, mut p) = fixture(); let root = dir.path();
    assert_eq!(record(root, &p).unwrap()["active"], false);
    assert!(selected(root, &["store.rs".into()]).is_empty());
    assert!(review(root, &p.name, "forged").is_err());
    review(root, &p.name, &review_fingerprint(root,&p)).unwrap();
    assert_eq!(selected(root, &["store.rs".into()]).len(), 1);
    assert!(selected(root, &["ui.tsx".into()]).is_empty());
    assert_eq!(record(root, &p).unwrap()["active"], true, "identical draft retains review");
    p.procedure.push_str(" Check rollback.");
    assert_eq!(record(root, &p).unwrap()["active"], false);
    review(root, &p.name, &review_fingerprint(root,&p)).unwrap();
    let prior=review_fingerprint(root,&p);
    fs::write_atomic(root.join("AGENTS.md"), b"Transactions must use existing shared helpers.\n").unwrap();
    assert!(!active(root,&list(root).unwrap()[0]));
    assert!(review(root,&p.name,&prior).is_err());
    review(root,&p.name,&review_fingerprint(root,&p)).unwrap();
    fs::write_atomic(root.join("store.rs"), b"fn changed() {}\n").unwrap();
    assert!(selected(root, &["store.rs".into()]).is_empty());
    assert!(export(root, &p.name, "codex").is_err());
    assert!(record(root, &p).is_err());
}

#[test]
fn exports_are_native_idempotent_and_preserve_manual_edits() {
    let (dir, p) = fixture(); let root = dir.path();
    record(root, &p).unwrap(); review(root, &p.name, &review_fingerprint(root,&p)).unwrap();
    for host in ["claude", "codex"] {
        let out = export(root, &p.name, host).unwrap(); assert_eq!(out["changed"], 2);
        assert_eq!(export(root, &p.name, host).unwrap()["changed"], 0);
        assert!(is_agent(root,"mustard-database",host));
        assert!(!is_agent(root,"mustard-forged",host));
        let agent = root.join(out["agent"].as_str().unwrap());
        let skill = root.join(out["skill"].as_str().unwrap());
        if host == "codex" {
            let value: toml::Value = toml::from_str(&std::fs::read_to_string(&agent).unwrap()).unwrap();
            assert!(value.get("model").is_none(), "executor inherits the host model; no Claude model in Codex");
            assert!(value["developer_instructions"].as_str().unwrap().contains("blocked_by"));
        }
        fs::write_atomic(&skill, b"user procedure").unwrap();
        assert!(export(root, &p.name, host).unwrap_err().contains("manual-file-preserved"));
        assert_eq!(std::fs::read_to_string(skill).unwrap(), "user procedure");
        assert!(agent.exists());
    }
}

#[test]
fn native_sync_learns_roles_reuses_and_updates_without_model_calls() {
    let dir = tempfile::tempdir().unwrap(); let root = dir.path();
    let mut modules = Vec::new();
    for n in 0..5 {
        let path = format!("area{n}/storage.service.rs");
        std::fs::create_dir_all(root.join(format!("area{n}"))).unwrap();
        let code = "fn save() {}\n";
        fs::write_atomic(root.join(&path), code.as_bytes()).unwrap();
        modules.push(json!({"path":path,"analysis":{"content_sha256":digest(code.as_bytes())},"declarations":[{"name":"save","kind":"function","line":1,"end_line":1}]}));
    }
    let mut raw = json!({"modules":modules}); crate::domain::knowledge::enrich(&mut raw);
    project_map::write_text(root, &raw.to_string()).unwrap();
    let first = sync(root).unwrap(); assert_eq!(first["native_profiles"], 1); assert_eq!(first["model_calls"], 0);
    assert_eq!(sync(root).unwrap()["changed"], 0);
    let selected = selected(root, &["area1/storage.service.rs".into()]); assert_eq!(selected.len(), 1);
    assert_eq!(wave_skills(root, &["area1/storage.service.rs".into()]).len(), 1);
    fs::write_atomic(root.join("area0/storage.service.rs"), b"fn changed() {}\n").unwrap();
    assert!(!active(root, &selected[0]));
    // Sync withdraws that stale example and preserves the other current ones.
    assert_eq!(sync(root).unwrap()["changed"], 1);
    assert!(active(root, &list(root).unwrap()[0]));
}

#[test]
fn stale_owned_exports_are_withdrawn_but_manual_edits_survive() {
    let (dir,p)=fixture(); let root=dir.path();
    record(root,&p).unwrap(); review(root,&p.name,&review_fingerprint(root,&p)).unwrap();
    let out=export(root,&p.name,"claude").unwrap();
    let skill=root.join(out["skill"].as_str().unwrap());
    assert!(is_skill(root,"mustard-database",&skill));
    fs::write_atomic(root.join(out["agent"].as_str().unwrap()),b"manual agent").unwrap();
    fs::write_atomic(root.join("store.rs"),b"fn changed() {}\n").unwrap();
    assert!(!is_skill(root,"mustard-database",&skill));
    let withdrawn=export::withdraw_stale(root).unwrap();
    assert_eq!(withdrawn["removed"].as_array().unwrap().len(),1);
    assert!(!skill.exists());
    assert_eq!(withdrawn["manual_edits_preserved"].as_array().unwrap().len(),1);
    assert_eq!(std::fs::read_to_string(root.join(out["agent"].as_str().unwrap())).unwrap(),"manual agent");
}
