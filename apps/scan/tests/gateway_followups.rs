#[path="support/model.rs"]
mod model;
use mustard_core::domain::code_search::Request;
use mustard_core::domain::knowledge::investigation::Purpose;
use mustard_core::io::code_search;
use serde_json::json;

#[test]
fn an_area_survey_keeps_the_native_named_area_before_unrelated_validation_verbs() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),r#"{"language":{"text":"pt-BR","code":"en-US"}}"#).unwrap();
    std::fs::create_dir(root.join("QuartzPay")).unwrap();
    std::fs::write(root.join("QuartzPay/service.cs"),"class QuartzPayClient {\n public void Create() {}\n}\n").unwrap();
    std::fs::write(root.join("unrelated.cs"),"class PasswordService { public bool ValidatePassword() { return true; } }\n").unwrap();
    model::scan(root,&root.join(".claude"),&["--native"]);
    let request=Request{tool:"rg".into(),input:json!({"args":["-n","--with-filename","-i","quartzpay|absentvendor","."]}),intent:"Validar todo o fluxo do quartzpay e a viabilidade de outro provedor".into(),purpose:Purpose::Spec,choose:false};
    let answer=code_search::execute(root,root,root,&request,None).unwrap();let ctx=&answer.report["task_context"];
    assert!(ctx["cards"][0]["source"]["file"].as_str().unwrap().starts_with("QuartzPay/"),"{ctx}");
    assert_eq!(ctx["recommended_symbols"],json!([]));assert_eq!(answer.stdout,code_search::execute_native(root,&request).unwrap().stdout);
}

#[test]
fn an_area_survey_keeps_named_native_functions_when_intent_uses_generic_verbs() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
    std::fs::write(root.join("provider.cs"),"class Provider {\n public void CreateChargeAsync() {}\n public void CancelChargeAsync() {}\n public void DischargeAsync() {}\n}\n").unwrap();
    std::fs::write(root.join("unrelated.cs"),"class PasswordService { public bool ValidatePassword() { return true; } }\n").unwrap();
    model::scan(root,&root.join(".claude"),&["--native"]);
    let request=Request{tool:"rg".into(),input:json!({"args":["-n","--with-filename","CreateCharge|CancelCharge","."]}),intent:"validate readiness".into(),purpose:Purpose::Spec,choose:false};
    let answer=code_search::execute(root,root,root,&request,None).unwrap();let ctx=&answer.report["task_context"];
    for name in ["CreateChargeAsync","CancelChargeAsync"] {assert!(ctx["cards"].as_array().unwrap().iter().any(|card|card["name"]==name && card["initial_reference"]==true),"{ctx}");}
    assert_ne!(ctx["cards"][0]["name"],"ValidatePassword");
    assert_eq!(ctx["recommended_symbols"],json!([]));assert_eq!(answer.stdout,code_search::execute_native(root,&request).unwrap().stdout);
}

#[test]
fn current_scanned_resources_and_empty_declaration_files_do_not_request_another_scan() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
    std::fs::write(root.join("guide.md"),"# Guide\nquartz snapshot\n").unwrap();
    std::fs::write(root.join("empty.ts"),"// quartz snapshot\n").unwrap();
    model::scan(root,&root.join(".claude"),&["--native"]);
    for (file,line,text) in [("guide.md",2,"quartz snapshot"),("empty.ts",1,"// quartz snapshot")] {
        let result=mustard_core::io::knowledge::observations::record(root,root,&[(file.into(),line,text.into())]).unwrap();
        assert_eq!(result["new_facts"],1);assert_eq!(result["needs_scan"],false,"{result}");
    }
    std::fs::write(root.join("empty.ts"),"// changed quartz snapshot\n").unwrap();
    let changed=mustard_core::io::knowledge::observations::record(root,root,&[("empty.ts".into(),1,"// changed quartz snapshot".into())]).unwrap();
    assert_eq!(changed["needs_scan"],true,"{changed}");
}

#[test]
fn lowercase_compound_name_is_indexed_ranked_and_located_from_current_source() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
    std::fs::write(root.join("provider.cs"),"class QuartzPayClient {\n  public void Send() {}\n}\nclass Unrelated { public void Validate() {} }\n").unwrap();
    model::scan(root,&root.join(".claude"),&["--native"]);
    let request=Request{tool:"rg".into(),input:json!({"args":["-n","--with-filename","-i","quartzpay","."]}),intent:"inspect quartzpay".into(),purpose:Purpose::Spec,choose:false};
    let answer=code_search::execute(root,root,root,&request,None).unwrap();
    assert!(answer.report["task_context"]["cards"].as_array().unwrap().iter().any(|c|c["name"]=="QuartzPayClient" && c["initial_reference"]==true),"{}",answer.report);
    assert_eq!(answer.stdout,code_search::execute_native(root,&request).unwrap().stdout);
    assert_eq!(answer.report["remote_model_calls"],0);
}

#[test]
fn named_internal_functions_are_searchable_without_cross_scope_edges() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::write(root.join("mustard.json"),"{}").unwrap();
    for (file,source) in [
        ("a.py","def factory():\n    def inner(value: int) -> int:\n        \"\"\"quartz transformation\"\"\"\n        return value + 1\n    return inner(3)\ndef unrelated():\n    return inner(7)\n"),
        ("b.ts","export function factory() {\n  function inner(value: number): number { return value + 1; }\n  return inner(3);\n}\nexport function unrelated() { return inner(7); }\n"),
        ("c.rs","pub fn factory() -> i32 {\n  fn inner(value: i32) -> i32 { value + 1 }\n  inner(3)\n}\nfn unrelated() -> i32 { inner(7) }\n"),
        ("d.cs","class Example {\n  int factory() {\n    int inner(int value) { return value + 1; }\n    return inner(3);\n  }\n  int unrelated() { return inner(7); }\n}\n"),
    ] {std::fs::write(root.join(file),source).unwrap();}
    let (map,_)=model::scan(root,&root.join(".claude"),&[]);
    for file in ["a.py","b.ts","c.rs","d.cs"] {
        let module=map["modules"].as_array().unwrap().iter().find(|m|m["path"]==file).unwrap();
        let cards=module["analysis"]["knowledge"]["cards"].as_array().unwrap();
        let inner=cards.iter().find(|c|c["name"]=="inner").unwrap_or_else(||panic!("named local function indexed in {file}: {module}"));
        assert_eq!(inner["syntax"]["lexical_scope"]["name"],"factory");
        assert!(!cards.iter().find(|c|c["name"]=="unrelated").unwrap()["outgoing"].as_array().unwrap().iter().any(|e|e["target"]==inner["id"]),"local helper leaked: {module}");
        let request=Request{tool:"rg".into(),input:json!({"args":["-n","--with-filename","inner",file]}),intent:"inspect inner transformation".into(),purpose:Purpose::Implement,choose:false};
        let answer=code_search::execute(root,root,root,&request,None).unwrap();
        assert!(answer.report["task_context"]["cards"].as_array().unwrap().iter().any(|c|c["id"]==inner["id"] && c["initial_source_excerpt"]==true),"{}",answer.report);
        assert_eq!(answer.stdout,code_search::execute_native(root,&request).unwrap().stdout);
        assert_eq!(answer.report["remote_model_calls"],0);
    }
}

#[test]
fn named_arrow_values_are_indexed_and_global_function_semantics_are_preserved() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
    std::fs::write(root.join("a.ts"),"function factory() {\n  const callback = (value: number) => value + 1;\n  return callback(1);\n}\n").unwrap();
    std::fs::write(root.join("b.php"),"<?php\nfunction factory() {\n  function inner($value) { return $value + 1; }\n}\nfunction use_global() { factory(); return inner(7); }\n").unwrap();
    let (map,_)=model::scan(root,&root.join(".claude"),&[]);let modules=map["modules"].as_array().unwrap();
    let arrow=modules.iter().find(|m|m["path"]=="a.ts").unwrap()["analysis"]["knowledge"]["cards"].as_array().unwrap().iter().find(|c|c["name"]=="callback").unwrap();
    assert_eq!(arrow["syntax"]["lexical_scope"]["name"],"factory");
    let cards=modules.iter().find(|m|m["path"]=="b.php").unwrap()["analysis"]["knowledge"]["cards"].as_array().unwrap();
    let inner=cards.iter().find(|c|c["name"]=="inner").unwrap();
    assert!(cards.iter().find(|c|c["name"]=="use_global").unwrap()["outgoing"].as_array().unwrap().iter().any(|e|e["target"]==inner["id"]));
}

#[test]
fn dependency_expansion_does_not_repeat_an_already_delivered_question_clue() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();std::fs::write(root.join("mustard.json"),"{}").unwrap();
    std::fs::write(root.join("a.rs"),"pub fn entry(value: i32) -> i32 { first(value) + second(value) }\nfn first(value: i32) -> i32 { value.saturating_add(1) }\nfn second(value: i32) -> i32 { value.saturating_add(2) }\n").unwrap();
    model::scan(root,&root.join(".claude"),&[]);
    let request=Request{tool:"rg".into(),input:json!({"args":["-n","--with-filename","entry","a.rs"]}),intent:"inspect entry saturating_add".into(),purpose:Purpose::Implement,choose:false};
    let answer=code_search::execute(root,root,root,&request,None).unwrap();let ctx=&answer.report["task_context"];
    assert_eq!(ctx["chain"]["appended_bodies"],1);
    assert_eq!(ctx["chain"]["steps"].as_array().unwrap().len(),2);
}

#[test]
fn complementary_native_search_uses_the_question_and_respects_original_file_scope() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::create_dir_all(root.join("allowed")).unwrap();std::fs::create_dir_all(root.join("outside")).unwrap();
    std::fs::write(root.join("mustard.json"),"{}").unwrap();
    std::fs::write(root.join("allowed/a.rs"),"fn ordinary_request() {}\nfn restore_quartz_revision() { reconcile_quartz(); }\n").unwrap();
    std::fs::write(root.join("outside/a.rs"),"fn restore_quartz_revision() { forbidden_source(); }\n").unwrap();
    for at in 0..12 {std::fs::write(root.join(format!("allowed/noise{at}.rs")),format!("fn ordinary_request{at}() {{}}\n")).unwrap();}
    model::scan(root,&root.join(".claude"),&[]);
    let request=Request{tool:"rg".into(),input:json!({"args":["--sort=path","-n","--with-filename","ordinary","allowed"]}),intent:"restore quartz revision".into(),purpose:Purpose::Implement,choose:false};
    let answer=code_search::execute(root,root,root,&request,None).unwrap();let ctx=&answer.report["task_context"];
    assert_eq!(ctx["complementary_discovery"]["status"],"native-question-probe");
    assert!(ctx["complementary_discovery"]["added_owners"].as_u64().unwrap()>0);
    assert!(ctx["cards"].as_array().unwrap().iter().any(|c|c["name"]=="restore_quartz_revision"));
    assert!(ctx["cards"].as_array().unwrap().iter().all(|c|c["source"]["file"].as_str().unwrap().starts_with("allowed/")));
    assert_eq!(answer.stdout,code_search::execute_native(root,&request).unwrap().stdout);
    assert_eq!(answer.report["remote_model_calls"],0);
}

#[test]
fn complementary_probe_joins_native_area_and_question_before_source_admission() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::create_dir_all(root.join("z/QuartzPay")).unwrap();
    std::fs::write(root.join("mustard.json"),"{}").unwrap();
    for n in 0..110 {
        std::fs::write(root.join(format!("event_{n:03}.rs")),format!("/// Authenticate event.\nfn handle_event_{n}() {{ authenticate_account(); }}\n")).unwrap();
    }
    std::fs::write(root.join("z/QuartzPay/entry.rs"),"fn handle_event() { persist_checkpoint(); }\nfn persist_checkpoint() { store_quartz(); }\n").unwrap();
    model::scan(root,&root.join(".claude"),&["--native"]);
    let request=Request{tool:"rg".into(),input:json!({"args":["--sort=path","-n","--with-filename","event","."]}),
        intent:"authenticate QuartzPay event checkpoint".into(),purpose:Purpose::Implement,choose:false};
    let answer=code_search::execute(root,root,root,&request,None).unwrap();let ctx=&answer.report["task_context"];
    assert!(ctx["complementary_discovery"]["partial"].as_bool().unwrap());
    assert!(ctx["complementary_discovery"]["added_owner_ids"].as_array().unwrap().iter()
        .any(|id|id.as_str().unwrap().ends_with(":persist_checkpoint")),"{}",ctx["complementary_discovery"]);
    assert!(ctx["cards"].as_array().unwrap().iter().any(|card|card["name"]=="persist_checkpoint" && card["initial_source_excerpt"]==true));
    assert_eq!(answer.stdout,code_search::execute_native(root,&request).unwrap().stdout);
    assert_eq!(answer.report["remote_model_calls"],0);
}

#[test]
fn declaration_ranges_remain_visible_but_cannot_spend_implementation_coverage() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::write(root.join("mustard.json"),"{}").unwrap();
    std::fs::write(root.join("a.cs"),"public interface IReceiver {\n    bool ReceiveEvent(string envelope);\n}\n").unwrap();
    std::fs::write(root.join("z.cs"),"public class Receiver : IReceiver {\n    public bool ReceiveEvent(string envelope) { return Persist(envelope); }\n    private bool Persist(string envelope) { return envelope.Length > 0; }\n}\n").unwrap();
    model::scan(root,&root.join(".claude"),&["--native"]);
    let request=Request{tool:"rg".into(),input:json!({"args":["--sort=path","-n","--with-filename","Event","."]}),
        intent:"receive event envelope".into(),purpose:Purpose::Implement,choose:false};
    let answer=code_search::execute(root,root,root,&request,None).unwrap();let cards=answer.report["task_context"]["cards"].as_array().unwrap();
    assert!(cards.iter().any(|c|c["name"]=="ReceiveEvent" && c["source"]["file"]=="a.cs" && c["source_role"]=="declaration-only" && c["initial_reference"]==true));
    assert!(cards.iter().any(|c|c["name"]=="ReceiveEvent" && c["source"]["file"]=="z.cs" && c["initial_source_excerpt"]==true));
    assert_eq!(answer.stdout,code_search::execute_native(root,&request).unwrap().stdout);
    assert_eq!(answer.report["remote_model_calls"],0);
}

#[test]
fn migrated_structural_index_requires_refresh_even_for_previously_learned_sources_or_empty_hits() {
    for empty_hits in [false,true] {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();
        std::fs::write(root.join("mustard.json"),"{}").unwrap();
        std::fs::write(root.join("entry.rs"),"pub fn entry() {}\n").unwrap();
        model::scan(root,&root.join(".claude"),&[]);
        let hits=vec![("entry.rs".into(),1,"pub fn entry() {}".into())];
        let before=mustard_core::io::knowledge::observations::record(root,root,&hits).unwrap();assert_eq!(before["needs_scan"],false);
        let conn=rusqlite::Connection::open(root.join(".claude/grain.db")).unwrap();
        conn.execute("UPDATE blocks SET version=15 WHERE name='decls'",[]).unwrap();drop(conn);
        let missing=mustard_core::io::knowledge::observations::record(root,root,if empty_hits{&[]}else{&hits}).unwrap();
        assert_eq!(missing["needs_scan"],true);
        model::scan(root,&root.join(".claude"),&[]);
        let after=mustard_core::io::knowledge::observations::record(root,root,&hits).unwrap();assert_eq!(after["needs_scan"],false);
    }
}

#[test]
fn exact_entry_follows_current_dependency_with_no_query_word_and_preserves_native_result() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("mustard.json"),"{}").unwrap();
    std::fs::write(root.join("src/entry.rs"),"pub fn entry(value: i32) -> i32 { distant(value) }\nfn distant(value: i32) -> i32 { value.saturating_add(1) }\n").unwrap();
    model::scan(root,&root.join(".claude"),&[]);
    let request=Request{tool:"rg".into(),input:json!({"args":["-n","--with-filename","entry","src"]}),intent:"inspect entry saturating_add".into(),purpose:Purpose::Implement,choose:false};
    let answer=code_search::execute(root,root,root,&request,None).unwrap();
    let cards=answer.report["task_context"]["cards"].as_array().unwrap();
    assert!(cards.iter().any(|c|c["name"]=="distant" && c["initial_source_excerpt"]==true),"{}",answer.report);
    assert!(answer.report["task_context"]["chain"]["steps"].as_array().unwrap().iter().any(|s|s["to"].as_str().unwrap().ends_with(":distant")));
    let narrow=Request{intent:"inspect entry".into(),..request.clone()};
    let narrow=code_search::execute(root,root,root,&narrow,None).unwrap();
    assert!(narrow.report["task_context"]["cards"].as_array().unwrap().iter().any(|c|c["name"]=="distant" && c["initial_reference"]==true && c["initial_source_excerpt"]==false));
    let native=code_search::execute_native(root,&request).unwrap();assert_eq!(native.stdout,answer.stdout);
    assert_eq!(answer.report["remote_model_calls"],0);
    let scoped=Request{input:json!({"args":["-n","entry","src/entry.rs"]}),..request};
    let scoped=code_search::execute(root,root,root,&scoped,None).unwrap();
    assert!(scoped.report["task_context"]["chain"]["steps"].as_array().unwrap().iter().all(|s|s["target_source"]["file"]=="src/entry.rs"));
}

#[test]
fn declared_parameters_and_return_types_are_source_ranges_in_multiple_grammars() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::write(root.join("mustard.json"),"{}").unwrap();
    for (file,source) in [("a.rs","pub fn example(value: i32) -> i32 { value }\n"),
        ("b.py","def example(value: int) -> int:\n    return value\n"),
        ("c.ts","export function example(value: number): number { return value; }\n")] {
        std::fs::write(root.join(file),source).unwrap();
    }
    let (map,_)=model::scan(root,&root.join(".claude"),&[]);
    for module in map["modules"].as_array().unwrap().iter().filter(|m|["a.rs","b.py","c.ts"].contains(&m["path"].as_str().unwrap_or_default())) {
        let card=module["analysis"]["knowledge"]["cards"].as_array().unwrap().iter().find(|c|c["name"]=="example").unwrap();
        let file=root.join(module["path"].as_str().unwrap());let source=std::fs::read_to_string(file).unwrap();
        let syntax=card["syntax"].as_object().unwrap();
        assert!(syntax.contains_key("parameters"),"{}",card);
        assert!(syntax.contains_key("return_type") || syntax.contains_key("declared_type"),"{}",card);
        for field in syntax.values() {
            let start=field["start_byte"].as_u64().unwrap() as usize;let end=field["end_byte"].as_u64().unwrap() as usize;
            assert_eq!(source.get(start..end).unwrap(),field["text"].as_str().unwrap());
        }
    }
}

#[test]
fn a_relevant_owner_after_many_native_hits_is_crossed_and_returned() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::write(root.join("mustard.json"),"{}").unwrap();
    let mut source=String::from("pub fn unrelated() {\n");
    for n in 0..600 {source.push_str(&format!("let marker_{n}=\"beacon\";\n"));}
    source.push_str("}\n/// Persists the revision snapshot.\npub fn persist_revision() { let beacon=1; }\n");
    std::fs::write(root.join("entry.rs"),source).unwrap();model::scan(root,&root.join(".claude"),&["--native"]);
    let request=Request{tool:"rg".into(),input:json!({"args":["-n","--with-filename","beacon","entry.rs"]}),intent:"persist revision snapshot".into(),purpose:Purpose::Implement,choose:false};
    let answer=code_search::execute(root,root,root,&request,None).unwrap();
    assert_eq!(answer.report["evidence"]["omitted_occurrences"],0);
    assert!(answer.report["evidence"]["current_owner_ids"].as_array().unwrap().iter().any(|id|id.as_str().unwrap().ends_with(":persist_revision")));
    assert!(answer.report["task_context"]["cards"].as_array().unwrap().iter().any(|c|c["name"]=="persist_revision"));
    assert_eq!(answer.stdout,code_search::execute_native(root,&request).unwrap().stdout);
}

#[test]
fn broad_native_owners_cannot_fill_the_pool_before_the_question_contributes() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::write(root.join("mustard.json"),"{}").unwrap();
    let source=(0..600).map(|n|format!("pub fn noise_{n}() {{ let beacon=1; }}\n")).collect::<String>();
    std::fs::write(root.join("a.rs"),source).unwrap();
    std::fs::write(root.join("z.rs"),"/// Restore the revision ledger checkpoint.\npub fn restore_checkpoint() { let beacon=2; }\n").unwrap();
    model::scan(root,&root.join(".claude"),&["--native"]);
    let request=Request{tool:"rg".into(),input:json!({"args":["--sort=path","-n","--with-filename","beacon","."]}),intent:"restore revision ledger checkpoint".into(),purpose:Purpose::Implement,choose:false};
    let answer=code_search::execute(root,root,root,&request,None).unwrap();
    assert!(answer.report["task_context"]["cards"].as_array().unwrap().iter().any(|c|c["name"]=="restore_checkpoint" && c["initial_source_excerpt"]==true),"{}",answer.report["task_context"]);
    assert_eq!(answer.report["remote_model_calls"],0);
}

#[test]
fn one_unavailable_source_does_not_discard_other_current_task_evidence() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::write(root.join("mustard.json"),"{}").unwrap();
    std::fs::write(root.join("a.py"),"secret = 'fixture-placeholder'\ndef hidden():\n    return 'beacon'\n").unwrap();
    std::fs::write(root.join("b.rs"),"/// Restore revision ledger.\npub fn restore() { let beacon=1; }\n").unwrap();
    model::scan(root,&root.join(".claude"),&["--native"]);
    let request=Request{tool:"rg".into(),input:json!({"args":["-n","--with-filename","beacon","."]}),intent:"restore revision ledger".into(),purpose:Purpose::Implement,choose:false};
    let answer=code_search::execute(root,root,root,&request,None).unwrap();let task=&answer.report["task_context"];
    assert_eq!(task["status"],"current-task-evidence","{task}");
    assert_eq!(task["investigation"]["partial"],true);
    assert!(task["cards"].as_array().unwrap().iter().any(|c|c["name"]=="restore" && c["initial_source_excerpt"]==true));
    assert!(task["cards"].as_array().unwrap().iter().all(|c|c["source"]["file"]!="a.py"));
    assert_eq!(answer.stdout,code_search::execute_native(root,&request).unwrap().stdout);
}

#[test]
fn selection_receives_ranked_candidates_before_the_visible_card_budget() {
    use mustard_core::domain::knowledge::selection::{Ambiguity,Decisions,SymbolSelector};
    struct ChooseLater;
    impl SymbolSelector for ChooseLater {
        fn select(&self,_:&str,groups:&[Ambiguity])->Decisions {
            assert_eq!(groups.len(),1);assert_eq!(groups[0].candidates.len(),24);
            let candidate=groups[0].candidates.iter().find(|card|card.name=="candidate_13").expect("ranked candidate lost before choice");
            assert!(groups[0].excerpts.contains_key(&candidate.id));
            Decisions{choices:std::iter::once((groups[0].key.clone(),candidate.id.clone())).collect(),usage:json!({"status":"fixture","remote_model_calls":0}),..Default::default()}
        }
    }
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    std::fs::write(root.join("mustard.json"),"{}").unwrap();
    let source=(0..32).map(|n|format!("/// Record ledger revision.\npub fn candidate_{n:02}() {{ let beacon=1; }}\n")).collect::<String>();
    std::fs::write(root.join("entry.rs"),source).unwrap();model::scan(root,&root.join(".claude"),&["--native"]);
    let request=Request{tool:"rg".into(),input:json!({"args":["-n","--with-filename","beacon","entry.rs"]}),intent:"record ledger revision".into(),purpose:Purpose::Implement,choose:true};
    let answer=code_search::execute(root,root,root,&request,Some(&ChooseLater)).unwrap();let task=&answer.report["task_context"];
    assert!(task["cards"].as_array().unwrap().iter().any(|c|c["name"]=="candidate_13" && c["recommended"]==true && c["initial_source_excerpt"]==true),"{task}");
    assert!(task.get("task_candidates").is_none());assert_eq!(task["decision_candidates"]["count"],24);
    assert_eq!(answer.stdout,code_search::execute_native(root,&request).unwrap().stdout);
}
