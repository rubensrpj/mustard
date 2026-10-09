#[path="support/model.rs"]
mod model;
use mustard_core::domain::code_search::Request;
use mustard_core::domain::knowledge::investigation::Purpose;
use mustard_core::io::code_search;
use serde_json::json;

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
    std::fs::write(root.join("src/entry.rs"),"pub fn entry(value: i32) -> i32 { distant(value) }\nfn distant(value: i32) -> i32 { value + 1 }\n").unwrap();
    model::scan(root,&root.join(".claude"),&[]);
    let request=Request{tool:"rg".into(),input:json!({"args":["-n","--with-filename","entry","src"]}),intent:"inspect entry".into(),purpose:Purpose::Implement,choose:false};
    let answer=code_search::execute(root,root,root,&request,None).unwrap();
    let cards=answer.report["task_context"]["cards"].as_array().unwrap();
    assert!(cards.iter().any(|c|c["name"]=="distant" && c["initial_source_excerpt"]==true),"{}",answer.report);
    assert!(answer.report["task_context"]["chain"]["steps"].as_array().unwrap().iter().any(|s|s["to"].as_str().unwrap().ends_with(":distant")));
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
